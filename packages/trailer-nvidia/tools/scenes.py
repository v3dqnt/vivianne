import sys
# Run after changing a scene's start or length: rewrites index.html's hosts and timeline.
# Rewrites the body of the trailer's index.html from a scene list.
# Each scene: (id, start, duration). Scenes after f03 arrive out of a blur.
SCENES = [
    ("f01-open", 0, 4.05),
    ("f02-engine", 3.6, 8),
    ("f03-router", 11.2, 10),
    ("f04-threads", 20.8, 5),
    ("f04-agents", 25.4, 8.5),
    ("f04-company", 33.5, 8.5),
    ("f05-memory", 41.6, 8.5),
    ("f06-world", 49.7, 8.2),
    ("f07-instant", 57.45, 7.2),
    ("f08-ui", 64.2, 7),
]
END = max(s + d for _, s, d in SCENES)
import os
p = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'index.html')
s = open(p, encoding='utf-8').read()
a = s.index('<body>') + len('<body>')
b = s.index('</body>')
hosts = []
for i, (sid, start, dur) in enumerate(SCENES):
    overlap = '' if sid == 'f01-open' else ' data-layout-allow-overlap'
    hosts.append(f'      <div id="{sid}"{overlap} data-composition-id="{sid}" data-composition-src="compositions/{sid}.html" data-start="{start}" data-duration="{dur}" data-track-index="{i + 1}" data-track-kind="graphics" data-width="1920" data-height="1080"></div>')
arrivals = '\n'.join(f'      arrive("{sid}", {start});' for sid, start, _ in SCENES[3:])
body = f'''
    <div id="root" data-composition-id="main" data-start="0" data-duration="{END:g}" data-width="1920" data-height="1080">
{chr(10).join(hosts)}
      <div id="grain" class="clip" data-start="0" data-duration="{END:g}" data-track-index="20"></div>
    </div>
    <script>
      const tl = gsap.timeline({{ paused: true }});
      // Every scene arrives out of a soft blur while the one before it is
      // still leaving: no hard cuts, no white flashes between scenes.
      const arrive = (id, t, d = 0.5) => tl.fromTo("#" + id, {{ opacity: 0, filter: "blur(14px)", scale: 0.985 }}, {{ opacity: 1, filter: "blur(0px)", scale: 1, duration: d, ease: "power2.out", immediateRender: false }}, t);
      arrive("f02-engine", 3.6, 0.45);
      // 02 → 03: a push to the right; the stream gives way to the settings beside it.
      tl.fromTo("#f03-router", {{ xPercent: 100 }}, {{ xPercent: 0, duration: 0.7, ease: "power3.inOut", immediateRender: false }}, 11.2);
      tl.fromTo("#f02-engine", {{ xPercent: 0 }}, {{ xPercent: -35, duration: 0.7, ease: "power3.inOut", immediateRender: false }}, 11.2);
{arrivals}
      window.__timelines["main"] = tl;
    </script>
  '''
s = s[:a] + body + s[b:]
open(p, 'w', encoding='utf-8', newline='').write(s)
print('end', END)
