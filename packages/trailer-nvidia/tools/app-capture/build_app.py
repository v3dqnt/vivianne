"""Turn captured gui-next states into trailer assets.

  out/app-*.html + out/app-*.css  ->  trailer-day/assets/app/app.css
                                       trailer-day/assets/app/fonts/*
                                       trailer-day/compositions/app-states.json  (id -> html)

The HTML/CSS is the app's own render output (React + Emotion), so the
trailer shows the real interface rather than a redraw of it. This script only
makes it safe to embed: global rules are scoped to `.vapp`, unused themes and
KaTeX are dropped, fonts are bundled, and ids are stripped so several states
can share one document.
"""
import json, re, shutil, sys
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "out"
PROJECT = Path(sys.argv[1])
GUI_SRC = Path(sys.argv[2])  # packages/gui-next
# Each further arg is <template>:<output>:<states>, e.g.
#   app.tpl.html:app.html:a,b,c   s4.tpl.html:s4-modes.html:th,co,tm
# All of them share one stylesheet built from every state named.
JOBS = [a.split(":") for a in sys.argv[3:]]
if len(JOBS) == 1 and len(JOBS[0]) == 1:  # old form: just the states
    JOBS = [["app.tpl.html", "app.html", JOBS[0][0]]]
JOBS = [(t, o, [x for x in st.split(",") if x]) for t, o, st in JOBS]
# A template can also place states itself with {{STATE:x}} tokens.
TOKEN = re.compile(r"\{\{STATE:([a-z0-9]+)\}\}")
JOBS = [(t, o, st + [x for x in dict.fromkeys(TOKEN.findall((HERE / t).read_text(encoding="utf-8"))) if x not in st]) for t, o, st in JOBS]
STATES = [x for _, _, st in JOBS for x in st]

def top_level_rules(css):
    out, cur, depth = [], "", 0
    for ch in css:
        cur += ch
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                out.append(cur.strip())
                cur = ""
    return out

GLOBAL_MAP = {
    "html, body, #root": ".vapp",
    "html *": ".vapp *",
    "html ::-webkit-scrollbar": ".vapp ::-webkit-scrollbar",
    "html": ".vapp",
    "body": ".vapp",
    "body::backdrop": None,
}

# Night states (named n…) are captured in the dark theme: their page-level
# rules are scoped to .vapp.vapp-dark so they never repaint the light states.
DARK = lambda st: st.startswith("n")

def keep(rule, dark=False):
    sel = rule.split("{", 1)[0].strip()
    # KaTeX (the chat's maths) is kept, fonts and all.
    if sel.startswith("@font-face"):
        return rule
    m = re.match(r"\.dockview-theme-([a-z-]+)", sel)
    if m and m.group(1) not in ("light", "dark"):
        return None
    if sel == ":root" and dark:
        return ".vapp.vapp-dark {" + rule.split("{", 1)[1]
    if sel in GLOBAL_MAP:
        new = GLOBAL_MAP[sel]
        if new and dark:
            new = new.replace(".vapp", ".vapp.vapp-dark")
        return None if new is None else new + rule[len(rule.split("{", 1)[0]):].replace(rule.split("{",1)[0], "", 0) if False else (None if new is None else new + " {" + rule.split("{", 1)[1])
    # Any other page-level theme rule from a dark state (the dock's panel
    # styling, for one) applies inside dark states only. Component classes
    # (.css-*) are already unique per theme.
    if dark and not sel.startswith(("@", ".css-")):
        parts = [".vapp.vapp-dark " + p.strip() for p in sel.split(",")]
        return ", ".join(parts) + " {" + rule.split("{", 1)[1]
    if sel.startswith("input, textarea"):
        parts = [".vapp " + p.strip() for p in sel.split(",")]
        return ", ".join(parts) + " {" + rule.split("{", 1)[1]
    return rule

seen, rules = set(), []
for s in STATES:
    for r in top_level_rules((OUT / f"app-{s}.css").read_text(encoding="utf-8")):
        k = keep(r, DARK(s))
        if k and k not in seen:
            seen.add(k)
            rules.append(k)

css = "\n".join(rules)
# The renderer seeks CSS animations, but only finite ones: cap the app's
# infinite spinners/pulses at a count that outlasts the film.
css = re.sub(r"\binfinite\b", "60", css)
# Transitions run on the browser clock and cannot be seeked: the film animates
# state changes itself, so the app's own transitions are dropped.
css = re.sub(r"(?<![w-])transition(-[a-z-]+)?\s*:[^;}]*;?", "", css)
# Fonts: bundle Geist + Geist Mono next to the stylesheet.
fonts_dir = PROJECT / "assets" / "app" / "fonts"
fonts_dir.mkdir(parents=True, exist_ok=True)
for url in set(re.findall(r'url\("(/src/assets/fonts/[^"]+)"\)', css)):
    src = GUI_SRC / url.lstrip("/")
    shutil.copy(src, fonts_dir / src.name)
    css = css.replace(f'url("{url}")', f'url("fonts/{src.name}")')
# KaTeX's fonts, from wherever node_modules resolved them.
for url in set(re.findall(r'url\("(/node_modules/katex/dist/fonts/[^"]+)"\)', css)):
    rel = url.lstrip("/")
    src = next((c for c in (GUI_SRC / rel, GUI_SRC.parent.parent / rel) if c.exists()), None)
    if src:
        (fonts_dir / "katex").mkdir(exist_ok=True)
        shutil.copy(src, fonts_dir / "katex" / src.name)
        css = css.replace(f'url("{url}")', f'url("fonts/katex/{src.name}")')
leftover = re.findall(r'url\("(/[^"]+)"\)', css)
(PROJECT / "assets" / "app" / "app.css").write_text(
    "/* Generated by build_app.py from the live gui-next render. Do not edit. */\n" + css, encoding="utf-8"
)

for tpl_name, out_name, job_states in JOBS:
    states = {}
    for st in job_states:
        html = (OUT / f"app-{st}.html").read_text(encoding="utf-8")
        html = html.replace('<div id="root">', '<div class="vapp vapp-dark">' if DARK(st) else '<div class="vapp">', 1)
        # Ids an SVG points at (masks, clip paths, gradients) are kept, made
        # unique per state; every other id goes, so states can share a document.
        refs = set(re.findall(r"url\(#([^)\"']+)\)", html)) | set(re.findall(r'href="#([^"]+)"', html))
        def _id(m):
            v = m.group(2)
            return f' id="{v}-{st}"' if m.group(1) == "id" and v in refs else ""
        html = re.sub(r'\s(id|aria-labelledby|aria-describedby|aria-controls|for)="([^"]*)"', _id, html)
        for v in refs:
            html = html.replace(f"url(#{v})", f"url(#{v}-{st})").replace(f'href="#{v}"', f'href="#{v}-{st}"')
        html = re.sub(r"(?<![w-])transition(-[a-z-]+)?\s*:[^;\"]*;?", "", html)
        states[st] = html
    tpl = (HERE / tpl_name).read_text(encoding="utf-8")
    block = lambda k: f'<div class="ap-state" data-state="{k}">{states[k]}</div>'
    tokens = set(TOKEN.findall(tpl))
    out = tpl.replace("{{STATES}}", "\n".join(block(k) for k in states if k not in tokens))
    out = TOKEN.sub(lambda m: block(m.group(1)), out)
    (PROJECT / "compositions" / out_name).write_text(out, encoding="utf-8")
    print(f"{out_name}: states {', '.join(f'{k}={len(v)//1024}KB' for k, v in states.items())}")
print(f"{len(rules)} rules, {len(css)//1024} KB css; unresolved urls: {leftover[:5]}")
