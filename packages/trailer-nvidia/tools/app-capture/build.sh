#!/bin/sh
# Rebuild every captured scene (app.css is the union of all states, so build them together).
cd "$(dirname "$0")" && python build_app.py ../.. ../../../gui-next \
  f02-engine.tpl.html:f02-engine.html:e1,e2 \
  f03-router.tpl.html:f03-router.html:r0,r1 \
  f04-threads.tpl.html:f04-threads.html:t1 \
  f04-agents.tpl.html:f04-agents.html:a2 \
  f04-company.tpl.html:f04-company.html:c1,c2,c3,w3 \
  f05-memory.tpl.html:f05-memory.html:m5 \
  f06-world.tpl.html:f06-world.html:v1 \
  f07-instant.tpl.html:f07-instant.html:e1
# Frame 8 was captured after the rest: it has its own stylesheet, app-f08.css.
CSS=app-f08.css python build_app.py ../.. ../../../gui-next \
  f08-mods.tpl.html:f08-mods.html:b1,b2,b4,b3
