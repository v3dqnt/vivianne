//! Driving the embedded live webview with the same surface camofox exposes.
//!
//! ## Why this exists
//!
//! The preview was a ~3fps stream of PNGs that could not be clicked, because
//! camofox is headless Firefox behind REST: it renders nothing locally and has
//! no streaming route, so a picture every ~300ms was the ceiling.
//!
//! `live_browser` fixed the *viewing* half by embedding a real webview. That
//! alone would have left two browsers with two cookie jars, silently diverging:
//! the agent signs into a site in camofox and the page you are looking at still
//! shows a login screen. Two sessions pretending to be one is worse than a slow
//! screenshot.
//!
//! So the agent drives the same webview the user is looking at. One browser,
//! one session, live, interactive from both sides.
//!
//! ## How the element refs work
//!
//! camofox hands the model an accessibility tree with stable refs (`e1`, `e2`)
//! and acts on them server-side. Here the equivalent is built in the page: the
//! snapshot walks the DOM, stamps `data-viv-ref` on each interactive element,
//! and returns matching lines. A later click looks the element up by that
//! attribute.
//!
//! Refs are therefore only valid until the next snapshot — exactly the contract
//! camofox already has, and the one the `camoufox-browser` skill already tells
//! the model about, so nothing the model has learned changes.

use crate::live_browser::eval_json;

/// Walks the DOM and returns a compact interactive-element listing.
///
/// Filtered to what can actually be acted on, because a full DOM dump of a real
/// page is tens of thousands of nodes and would bury the handful that matter.
/// Accessible names come from aria-label, then the labelling element, then
/// trimmed text — the same order a screen reader resolves them in.
const SNAPSHOT_JS: &str = r#"
(() => {
  const INTERACTIVE = 'a,button,input,select,textarea,summary,[role=button],[role=link],[role=textbox],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[contenteditable=true],[onclick]';
  const seen = document.querySelectorAll('[data-viv-ref]');
  for (const el of seen) el.removeAttribute('data-viv-ref');

  const visible = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) return false;
    const s = getComputedStyle(el);
    return s.visibility !== 'hidden' && s.display !== 'none' && s.opacity !== '0';
  };
  const name = (el) => {
    const aria = el.getAttribute('aria-label');
    if (aria) return aria.trim();
    const labelledby = el.getAttribute('aria-labelledby');
    if (labelledby) {
      const l = document.getElementById(labelledby);
      if (l) return (l.innerText || '').trim();
    }
    if (el.tagName === 'INPUT') {
      const ph = el.getAttribute('placeholder');
      if (ph) return ph.trim();
      if (el.labels && el.labels[0]) return (el.labels[0].innerText || '').trim();
      const v = el.getAttribute('value');
      if (v) return v.trim();
    }
    const alt = el.getAttribute('alt') || el.getAttribute('title');
    if (alt) return alt.trim();
    return (el.innerText || el.textContent || '').trim().slice(0, 120);
  };
  const role = (el) => {
    const r = el.getAttribute('role');
    if (r) return r;
    const t = el.tagName.toLowerCase();
    if (t === 'a') return 'link';
    if (t === 'button' || t === 'summary') return 'button';
    if (t === 'select') return 'combobox';
    if (t === 'textarea') return 'textbox';
    if (t === 'input') {
      const it = (el.getAttribute('type') || 'text').toLowerCase();
      if (it === 'checkbox' || it === 'radio' || it === 'submit' || it === 'button') return it;
      return 'textbox';
    }
    return 'generic';
  };

  const out = [];
  let n = 0;
  for (const el of document.querySelectorAll(INTERACTIVE)) {
    if (!visible(el)) continue;
    n += 1;
    const ref = 'e' + n;
    el.setAttribute('data-viv-ref', ref);
    const label = name(el).replace(/\s+/g, ' ');
    let line = '- ' + role(el) + ' "' + label + '" [' + ref + ']';
    if (el.value && el.tagName !== 'BUTTON') line += ' value="' + String(el.value).slice(0, 60) + '"';
    if (el.disabled) line += ' [disabled]';
    out.push(line);
    if (n >= 400) break;
  }
  // Page text, trimmed: the model needs to read the page, not only act on it.
  const bodyText = (document.body ? document.body.innerText : '').replace(/\n{3,}/g, '\n\n').slice(0, 12000);
  return JSON.stringify({
    url: location.href,
    title: document.title,
    refs: n,
    elements: out.join('\n'),
    text: bodyText,
  });
})()
"#;

/// Snapshot the live page: interactive elements plus readable text.
pub async fn snapshot(app: &tauri::AppHandle) -> Result<crate::camoufox::SnapshotResult, String> {
    let v = eval_json(app, SNAPSHOT_JS).await?;
    // `eval_with_callback` serialises the JS return value, so a string return
    // arrives as a JSON string that still has to be parsed.
    let parsed: serde_json::Value = match v.as_str() {
        Some(s) => serde_json::from_str(s).map_err(|e| format!("unreadable snapshot: {e}"))?,
        None => v,
    };
    let url = parsed.get("url").and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let title = parsed.get("title").and_then(|x| x.as_str()).unwrap_or_default();
    let refs = parsed.get("refs").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let elements = parsed.get("elements").and_then(|x| x.as_str()).unwrap_or_default();
    let text = parsed.get("text").and_then(|x| x.as_str()).unwrap_or_default();
    Ok(crate::camoufox::SnapshotResult {
        url,
        text: format!("{title}\n\n{elements}\n\n--- page text ---\n{text}"),
        refs_count: refs,
        // The JS already caps its own output, so there is no second page to
        // fetch — reporting otherwise would send the model chasing an offset
        // that does nothing.
        truncated: false,
        has_more: false,
        next_offset: None,
    })
}

/// JS-escape a string for embedding in a single-quoted literal.
fn js_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n").replace('\r', "")
}

/// Click the element a previous snapshot stamped with `ref`.
///
/// Dispatches the full press sequence rather than `.click()`, because plenty of
/// real UI listens for mousedown rather than the synthesised click, and focuses
/// first so a subsequent type lands in the right field.
pub async fn click(app: &tauri::AppHandle, r: &str) -> Result<String, String> {
    let js = format!(
        r#"(() => {{
  const el = document.querySelector('[data-viv-ref="{}"]');
  if (!el) return 'missing';
  el.scrollIntoView({{ block: 'center' }});
  try {{ el.focus(); }} catch (e) {{}}
  const o = {{ bubbles: true, cancelable: true, view: window }};
  el.dispatchEvent(new MouseEvent('mousedown', o));
  el.dispatchEvent(new MouseEvent('mouseup', o));
  el.dispatchEvent(new MouseEvent('click', o));
  return 'ok';
}})()"#,
        js_str(r)
    );
    match eval_json(app, &js).await?.as_str() {
        Some("missing") => Err(format!(
            "no element {r} on the page — refs only last until the next snapshot, so call browser_snapshot again"
        )),
        _ => Ok(format!("Clicked {r}.")),
    }
}

/// Fill a field, firing the events frameworks listen for.
///
/// Setting `.value` alone updates the DOM but not React/Vue state, which is why
/// the native setter is invoked and `input`/`change` dispatched explicitly —
/// otherwise the text appears and the app behaves as though the field is empty.
pub async fn type_text(app: &tauri::AppHandle, r: &str, text: &str, submit: bool) -> Result<String, String> {
    let js = format!(
        r#"(() => {{
  const el = document.querySelector('[data-viv-ref="{}"]');
  if (!el) return 'missing';
  try {{ el.focus(); }} catch (e) {{}}
  const v = '{}';
  if (el.isContentEditable) {{
    // Rich editors (compose boxes, chat inputs) have no .value.
    document.execCommand('selectAll', false);
    document.execCommand('insertText', false, v);
  }} else {{
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, 'value');
    if (setter && setter.set) setter.set.call(el, v); else el.value = v;
    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
  }}
  if ({}) {{
    el.dispatchEvent(new KeyboardEvent('keydown', {{ key: 'Enter', bubbles: true }}));
    if (el.form && el.form.requestSubmit) el.form.requestSubmit();
  }}
  return 'ok';
}})()"#,
        js_str(r),
        js_str(text),
        if submit { "true" } else { "false" }
    );
    match eval_json(app, &js).await?.as_str() {
        Some("missing") => Err(format!(
            "no element {r} on the page — refs only last until the next snapshot, so call browser_snapshot again"
        )),
        _ => Ok(format!("Typed into {r}.")),
    }
}

pub async fn press_key(app: &tauri::AppHandle, key: &str) -> Result<String, String> {
    let js = format!(
        r#"(() => {{
  const el = document.activeElement || document.body;
  const o = {{ key: '{}', bubbles: true, cancelable: true }};
  el.dispatchEvent(new KeyboardEvent('keydown', o));
  el.dispatchEvent(new KeyboardEvent('keyup', o));
  return 'ok';
}})()"#,
        js_str(key)
    );
    eval_json(app, &js).await?;
    Ok(format!("Pressed {key}."))
}

pub async fn scroll(app: &tauri::AppHandle, direction: &str, amount: i64) -> Result<String, String> {
    let (dx, dy) = match direction {
        "up" => (0, -amount),
        "down" => (0, amount),
        "left" => (-amount, 0),
        "right" => (amount, 0),
        other => return Err(format!("unknown direction '{other}' — use up, down, left or right")),
    };
    eval_json(app, &format!("(() => {{ window.scrollBy({dx}, {dy}); return 'ok'; }})()")).await?;
    Ok(format!("Scrolled {direction} {amount}px."))
}

/// Whether the live view is available to drive right now.
///
/// The routing question for every browser tool: use the webview the user is
/// actually looking at, or fall back to camofox. Asking the page rather than
/// tracking a flag means a webview that was closed, crashed, or never opened
/// all answer the same way.
pub async fn is_live(app: &tauri::AppHandle) -> bool {
    eval_json(app, "'ok'").await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_strings_are_escaped_so_a_quote_cannot_break_out() {
        // A page ref or typed text containing a quote would otherwise terminate
        // the literal and turn the rest into executable code.
        assert_eq!(js_str("it's"), "it\\'s");
        assert_eq!(js_str(r"back\slash"), r"back\\slash");
        assert_eq!(js_str("two\nlines"), "two\\nlines");
    }

    #[test]
    fn carriage_returns_are_dropped_not_escaped() {
        // A raw CR inside a JS string literal is a syntax error; \n alone is the
        // portable representation.
        assert_eq!(js_str("a\r\nb"), "a\\nb");
    }
}
