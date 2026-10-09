#!/usr/bin/env bash
# Differential test: `acu grep` must find exactly what `rg` finds.
# usage: tests/diff-vs-rg.sh <acu-binary> <directory-to-search>
# Needs ripgrep (rg). Lines longer than ~450 characters are excluded from the comparison because acu cuts lines at 500.
# acu treats CRLF as a line ending for `$` (as an agent expects); rg needs --crlf for that, so that check passes it.
set -u
ACU=${1:?acu binary}
ACU_Q=$(printf '%q' "$ACU")
DIR=${2:?directory}
fail=0
total=0

norm() { sed -e 's#\\#/#g' -e 's#^\./##' | awk 'length($0) <= 450' | sort; }

check() {
  local label=$1; shift
  local acu_args=$1; shift
  local rg_args=$1; shift
  total=$((total + 1))
  # shellcheck disable=SC2086
  local a r
  a=$(cd "$DIR" && eval "$ACU_Q grep -m 0 $acu_args" 2>/dev/null | norm)
  r=$(cd "$DIR" && eval "rg --hidden --no-messages --no-heading --line-number --color never --glob '!.git' $rg_args" 2>/dev/null | norm)
  if [ "$a" != "$r" ]; then
    fail=$((fail + 1))
    echo "DIFF  $label"
    diff <(echo "$a") <(echo "$r") | head -6
  fi
}

check "literal"            "-F 'useState' ."                      "-F 'useState' ."
check "literal icase"      "-F -i 'usestate' ."                   "-F -i 'usestate' ."
check "word"               "-w 'type' ."                          "-w 'type' ."
check "regex alt"          "'TODO|FIXME|HACK' ."                  "'TODO|FIXME|HACK' ."
check "regex class"        "'[A-Z][a-z]+State\\b' ."              "'[A-Z][a-z]+State\\b' ."
check "regex digits"       "'\\b[0-9]{3,4}px\\b' ."               "'\\b[0-9]{3,4}px\\b' ."
check "anchored start"     "'^import ' ."                         "'^import ' ."
check "anchored end"       "';\$' ."                              "--crlf ';\$' ."
check "dot star"           "'export .*function' ."                "'export .*function' ."
check "optional"           "'colou?r' ."                          "'colou?r' ."
check "empty alt branch"   "'foo(bar)?baz' ."                     "'foo(bar)?baz' ."
check "negated class"      "'[^a-z\\s]{12}' ."                    "'[^a-z\\s]{12}' ."
check "lazy"               "'<.+?>' ."                            "'<.+?>' ."
check "icase regex"        "-i 'error|warning' ."                 "-i 'error|warning' ."
check "multi -e"           "-e 'async' -e 'await' ."              "-e 'async' -e 'await' ."
check "escaped dot"        "'\\.tsx?\"' ."                        "'\\.tsx?\"' ."
check "unicode literal"    "-F '—' ."                             "-F '—' ."
check "no match"           "'zzqxjkvv_not_present' ."             "'zzqxjkvv_not_present' ."
check "type filter"        "--type ts 'interface ' ."             "-t ts 'interface ' ."
check "glob filter"        "-g '*.css' 'color' ."                 "-g '*.css' 'color' ."

echo "$((total - fail))/$total checks identical to rg"
[ "$fail" -eq 0 ]
