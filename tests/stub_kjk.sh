#!/bin/sh
# A stand-in for kjk's `installs` command, for the launcher's tests and
# screenshots. `installs --json` prints a fixed report: KOTOR found on Steam,
# KOTOR II refused, Jedi Academy missing, ffmpeg found. With
# STUB_KJK_READY=1 every game is found. `installs --json --check k1 /good`
# reports that folder as found, and any other check as refused. Anything
# else exits with 2. With STUB_KJK_SECONDS=n, anything else waits n seconds
# first and then exits with 0, as a game would.
if [ "$1" = "installs" ] && [ "$2" = "--json" ]; then
  if [ "$3" = "--check" ]; then
    if [ "$4" = "k1" ] && [ "$5" = "/good" ]; then
      echo '{"games":[{"game":"k1","status":"found","path":"/good","source":"settings"}]}'
    else
      echo "{\"games\":[{\"game\":\"$4\",\"status\":\"refused\",\"path\":\"$5\",\"source\":\"settings\",\"why\":\"no game files in this folder\"}]}"
    fi
  elif [ "$STUB_KJK_READY" = "1" ]; then
    echo '{"games":[{"game":"k1","status":"found","path":"/Games/Steam/steamapps/common/swkotor","source":"steam"},{"game":"tsl","status":"found","path":"/Games/GOG/Star Wars KOTOR2","source":"gog"},{"game":"ja","status":"found","path":"/Games/Jedi Academy","source":"settings"}],"ffmpeg":{"status":"found"}}'
  else
    echo '{"games":[{"game":"k1","status":"found","path":"/Games/Steam/steamapps/common/swkotor","source":"steam"},{"game":"tsl","status":"refused","path":"/Games/wrong","source":"settings","why":"no swkotor2.exe in this folder"},{"game":"ja","status":"missing"}],"ffmpeg":{"status":"found"}}'
  fi
  exit 0
fi
if [ -n "$STUB_KJK_SECONDS" ]; then
  sleep "$STUB_KJK_SECONDS"
  exit 0
fi
exit 2
