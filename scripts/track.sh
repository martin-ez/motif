#!/usr/bin/env bash
# scripts/track.sh — work tracking for motif.
#
# The ONLY supported way to read or change GitHub Issues in this repo. See AGENTS.md.
#
# Never call `gh issue` / `gh api` / `gh search` directly: the legacy issue-search
# index silently ignores `is:blocked` and `no:blocked-by` and returns blocked issues
# as if they were ready, with a 200 OK and no error. This script never touches the
# search index — it derives readiness locally from each issue's blockedBy payload,
# which is also read-your-writes consistent (the search index lags writes by seconds).
#
# Structured output -> stdout.  Progress and diagnostics -> stderr.
# Exit codes:
#   0  success
#   1  error — caller surfaces stderr verbatim and stops
#   2  claim contention — someone else holds it; pick a different issue
#
# Written for bash 3.2 (macOS /bin/bash): no associative arrays, no mapfile.

set -euo pipefail

ISSUE_FIELDS='number,title,state,url,labels,blockedBy,blocking,parent,subIssues,subIssuesSummary'
LIST_LIMIT="${TRACK_LIMIT:-200}"
TITLE_MAX="${TRACK_TITLE_MAX:-70}"
MIN_WRITE_GAP="${TRACK_MIN_WRITE_GAP:-1}"
STALE_HOURS="${TRACK_STALE_HOURS:-24}"

die()  { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
note() { printf '%s\n' "$*" >&2; }

usage() {
  cat >&2 <<'USAGE'
scripts/track.sh <command> [args] [--json]

  ready                       work that can be started right now
  blocked                     open work with open blockers, and what blocks it
  find <term>                 match issue titles, open and closed
  show <n>                    one issue in full, including claim state
  start <n>                   branch from main, then claim (see: claim needs a branch)
  claim <n> [--force]         take an issue (adds wip + a claim marker)
  mine                        issues this agent currently holds
  release <n> [--force]       give it back
  done <n> [-m MSG] [--force] close it, report what it unblocked
  add -t TITLE --area A --kind K --size S [-b BODY|-F FILE]
      [--blocked-by N,...] [--blocking N,...] [--parent N]
  dep <n> [--needs N] [--drop-needs N] [--child N] [--drop-child N]
  note <n> -m MSG             leave a comment on an issue
  graph                       dependency forest of open issues
  labels-init                 create/update the label taxonomy (idempotent)
  doctor                      check preconditions
  selftest --yes              full lifecycle smoke test on throwaway issues
USAGE
  exit 1
}

# ----------------------------------------------------------------- labels ---
LABEL_SPEC='area:engine|1f6feb|Audio engine, DSP, realtime core
area:seq|388bfd|Sequencer, patterns, timing
area:synth|58a6ff|Voices, oscillators, effects
area:ui|a5d6ff|Terminal UI, layout, input
area:io|0969da|MIDI, audio devices, file persistence
area:infra|8b949e|Build, CI, tooling, dependencies
kind:feat|2da44e|New capability
kind:bug|d1242f|Something is wrong
kind:chore|6e7781|Maintenance, refactor, dependency bumps
kind:spike|8250df|Time-boxed investigation, output is throwaway
size:s|ededed|Well under one agent session
size:m|d0d7de|About one agent session
size:l|afb8c1|Too big to claim — split into sub-issues first
wip|fbca04|Claimed by an agent. Set by track.sh claim only.
track:selftest|c5def5|Throwaway issue from track.sh selftest. Safe to delete.'

label_names() { printf '%s\n' "$LABEL_SPEC" | awk -F'|' 'NF{print $1}'; }
valid_label()  { label_names | grep -qx -- "$1"; }
label_values() { label_names | grep "^$1:" | sed "s/^$1://" | tr '\n' ' '; }

# --------------------------------------------------------------- identity ---
repo_key() {
  local main
  main="$(git worktree list 2>/dev/null | awk 'NR==1{print $1}')"
  [ -n "$main" ] || return 1
  printf '%s' "$main" | shasum | cut -c1-12
}

agent_id() {
  if [ -n "${MOTIF_AGENT:-}" ]; then validate_agent "$MOTIF_AGENT"; printf '%s' "$MOTIF_AGENT"; return 0; fi
  local br
  br="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
  case "$br" in
    ''|HEAD)     die "cannot determine agent id — set MOTIF_AGENT." ;;
    main|master) die "refusing to act as agent '$br'. Work on a branch, or set MOTIF_AGENT." ;;
  esac
  printf '%s' "$br"
}

# The claim marker parses the agent with [^ ]+, so whitespace would produce a
# marker that can never be matched back: the claim would look successful and be
# invisible to every other command.
validate_agent() {
  case "$1" in
    *[[:space:]]*) die "agent id '$1' contains whitespace; use [A-Za-z0-9._/-] only." ;;
    '')            die "agent id is empty." ;;
  esac
  return 0
}

# ------------------------------------------------- lock + write pacing ------
# mkdir is atomic on any POSIX filesystem (flock is unreliable on macOS).
# Held across read-then-write, this makes `claim` a genuine compare-and-swap.
# It also paces every content-generating request >=1s apart, globally across
# all agents on this machine, satisfying the 80/min and 500/hour secondary limit.
# Resolved on first use, not at load time: computing it eagerly makes --help and
# doctor -- the things you reach for when something is wrong -- fail outside a repo.
STATE_DIR=""
LOCK=""
STAMP=""
LOCK_HELD=0

state_init() {
  [ -n "$STATE_DIR" ] && return 0
  local k
  k="$(repo_key)" || die "not inside a git repository."
  STATE_DIR="${TMPDIR:-/tmp}/motif-track-$k"
  LOCK="$STATE_DIR/lock"
  STAMP="$STATE_DIR/last-write"
  return 0
}

lock_acquire() {
  [ "$LOCK_HELD" = 1 ] && return 0
  state_init
  mkdir -p "$STATE_DIR"
  local tries=0 holder=""
  until mkdir "$LOCK" 2>/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -gt 300 ]; then
      # A long hold is normal: labels-init and selftest legitimately keep the
      # lock for a minute across paced writes. Only break it once the recorded
      # holder is gone, and reset the counter afterwards -- breaking on every
      # subsequent tick would delete a live lock and let two callers hold it.
      holder="$(cat "$LOCK/pid" 2>/dev/null || true)"
      if [ -n "$holder" ] && kill -0 "$holder" 2>/dev/null; then
        note "waiting on live lock holder pid $holder …"
      else
        note "WARNING: breaking a lock whose holder (${holder:-unknown}) is gone"
        rm -rf "$LOCK"
      fi
      tries=0
    fi
    sleep 0.2
  done
  LOCK_HELD=1
  printf '%s' "$$" > "$LOCK/pid" 2>/dev/null || true
  return 0
}

lock_release() {
  [ "$LOCK_HELD" = 1 ] || return 0
  rm -rf "$LOCK"
  LOCK_HELD=0
  return 0
}
trap lock_release EXIT INT TERM

# Every mutating gh call goes through here. Never call `gh` directly for writes.
gh_write() {
  lock_acquire
  local now last delta rc
  now="$(date +%s)"
  last="$(cat "$STAMP" 2>/dev/null || echo 0)"
  delta=$(( now - last ))
  if [ "$delta" -lt "$MIN_WRITE_GAP" ]; then
    sleep $(( MIN_WRITE_GAP - delta ))
  fi
  rc=0
  gh "$@" || rc=$?
  date +%s > "$STAMP"
  return "$rc"
}

# ------------------------------------------------------------- jq library ---
JQ_LIB='
def _lbl($p): (.labels // []) | map(.name) | map(select(startswith($p)))
              | (.[0] // "") | ltrimstr($p);
def _has($n): (((.labels // []) | map(.name) | index($n)) != null);
def _trunc: ((.blockedBy.totalCount // 0) > ((.blockedBy.nodes // []) | length))
            or ((.subIssues.totalCount // 0) > ((.subIssues.nodes // []) | length));
def _openblk:  (.blockedBy.nodes // []) | map(select(.state == "OPEN"));
def _doneblk:  (.blockedBy.nodes // []) | map(select(.state == "CLOSED"));
def _openbing: (.blocking.nodes  // []) | map(select(.state == "OPEN"));
def _opensub:  (.subIssues.nodes // []) | map(select(.state == "OPEN"));

def shape: {
  num:       .number,
  title:     ((.title // "") | gsub("[\t\r\n]"; " ")),
  state:     .state,
  area:      _lbl("area:"),
  kind:      _lbl("kind:"),
  size:      _lbl("size:"),
  wip:       _has("wip"),
  blockers:  (_openblk  | map(.number)),
  done_deps: (_doneblk  | map(.number)),
  unblocks:  (_openbing | map(.number)),
  subs_open: (_opensub  | map(.number)),
  subs:      (.subIssuesSummary // {total: 0, completed: 0}),
  trunc:     _trunc,
  parent:    (if .parent == null then null
              else {num: .parent.number, title: .parent.title, state: .parent.state} end),
  url:       .url
};

def is_ready:   (.state == "OPEN") and (.wip | not) and (.trunc | not)
                and (.size != "l")
                and ((.blockers | length) == 0) and ((.subs_open | length) == 0);
def needs_split: (.state == "OPEN") and (.wip | not) and (.size == "l")
                and ((.blockers | length) == 0) and ((.subs_open | length) == 0);
def is_container: (.state == "OPEN") and ((.subs_open | length) > 0);
def is_blocked: (.state == "OPEN") and ((.blockers | length) > 0);

# Kahn peeling: repeatedly drop nodes whose remaining blockers are all satisfied.
# Whatever survives is in a cycle or downstream of one. Blockers are first
# restricted to issues we actually fetched, so an out-of-window blocker cannot
# masquerade as a cycle.
def cycle_nodes:
  ([.[] | .num]) as $known
  | [ .[] | {num: .num, blk: [ .blockers[] | select(IN($known[])) ]} ] as $init
  | ( reduce range(0; ($init | length) + 1) as $_ ($init;
        (map(select(.blk | length == 0) | .num)) as $free
        | if ($free | length) == 0 then .
          else [ .[] | select((.blk | length) > 0) | {num: .num, blk: (.blk - $free)} ]
          end) )
  | map(.num) | sort;
def has_cycle: ((cycle_nodes | length) > 0);

def top_blockers:
  [.[] | select(is_blocked) | .blockers[]]
  | group_by(.) | map({num: .[0], n: length}) | sort_by(-.n, .num);
'

# Claim ownership lives in HTML-comment markers on issue comments. They arrive on
# the same `gh issue view` call as everything else, so reading them is free.
JQ_CLAIM='
def markers:
  [ (.comments // [])[]
    | (.body | capture("<!-- track:(?<ev>claim|release|done) agent=(?<agent>[^ ]+) -->")?)
      as $m
    | select($m != null)
    | {ev: $m.ev, agent: $m.agent, at: .createdAt} ];
def holder:
  (markers | last) as $m
  | if $m == null or $m.ev != "claim" then null
    else {agent: $m.agent, since: $m.at} end;
'

# ------------------------------------------------------------ read paths ----
# The only three places this script reads issue data. No search query anywhere:
# `find` matches locally over fetch_all for the same reason readiness is derived
# locally — the legacy index answers `is:blocked` wrongly with a 200, and there is
# no reason to trust its title matching any further than that.
fetch_open()  { gh issue list --state open --limit "$LIST_LIMIT" --json "$ISSUE_FIELDS"; }
fetch_all()   { gh issue list --state all  --limit "$LIST_LIMIT" --json "$ISSUE_FIELDS"; }
fetch_issue() { gh issue view "$1" --json "$ISSUE_FIELDS,body,comments"; }

# Claim markers live on comments, which `fetch_open` does not carry. Fetching
# every open issue to find them would cost one call per issue; the wip label is
# already in the list payload, so the walk is bounded by the number of live
# claims instead of the size of the backlog.
claimed_issues() {
  local nums n
  nums="$(fetch_open | jq -r '.[] | select((.labels // []) | map(.name) | index("wip")) | .number')"
  for n in $nums; do
    fetch_issue "$n" | jq -c "$JQ_LIB$JQ_CLAIM"' shape + {claim: holder}'
  done
  return 0
}

# GNU date takes -d, BSD date takes -j -f. A timestamp that parses under neither
# yields 0, which every caller reads as "unknown age" and skips.
iso_epoch() {
  date -u -d "$1" +%s 2>/dev/null \
    || date -u -j -f '%Y-%m-%dT%H:%M:%SZ' "$1" +%s 2>/dev/null \
    || echo 0
}

# --------------------------------------------------------------- commands ---
cmd_ready() {
  local shaped payload total n nblocked nwip ncont tb cyc split trunc
  shaped="$(fetch_open | jq "$JQ_LIB"' [.[] | shape]')"
  total="$(printf '%s' "$shaped" | jq 'length')"
  payload="$(printf '%s' "$shaped" | jq "$JQ_LIB"'
    [.[] | select(is_ready)] | sort_by(-(.unblocks | length), .num)')"

  if [ "$AS_JSON" = 1 ]; then printf '%s\n' "$payload"; return 0; fi

  # A cycle can strand part of the backlog while other work is still ready, so
  # this is reported whether or not the queue is empty.
  cyc="$(printf '%s' "$shaped" | jq -r "$JQ_LIB"'cycle_nodes | map("#\(.)") | join(" ")')"

  n="$(printf '%s' "$payload" | jq 'length')"
  printf 'ready %s/%s open\n' "$n" "$total"

  if [ "$n" -gt 0 ]; then
    printf '%s' "$payload" | jq -r --argjson m "$TITLE_MAX" '
      .[] | [ .num, (.size // ""), (.area // ""), (.kind // ""), (.unblocks | length),
              (if (.title | length) > $m then (.title[0:$m] + "…") else .title end)
            ] | @tsv' \
    | awk -F'\t' '{
        u = ($5 + 0 > 0) ? sprintf("  (unblocks %d)", $5) : "";
        printf "  #%-4s %-1s  %-7s %-6s %s%s\n",
               $1, ($2 == "" ? "-" : $2), ($3 == "" ? "-" : $3), ($4 == "" ? "-" : $4), $6, u;
      }'
  else
    # Empty queue: say why, on stdout, where the agent will actually read it.
    # Containers are counted separately -- an open, unclaimed, unblocked issue
    # with open children is in neither of the other buckets.
    nblocked="$(printf '%s' "$shaped" | jq "$JQ_LIB"'[.[] | select(is_blocked)] | length')"
    nwip="$(printf '%s' "$shaped" | jq '[.[] | select(.wip)] | length')"
    ncont="$(printf '%s' "$shaped" | jq "$JQ_LIB"'[.[] | select(is_container)] | length')"
    printf '  nothing is ready. %s blocked, %s claimed, %s waiting on sub-issues.\n' \
      "$nblocked" "$nwip" "$ncont"
    tb="$(printf '%s' "$shaped" | jq -r "$JQ_LIB"'
          top_blockers[0:5] | map("#\(.num) (\(.n))") | join("  ")')"
    [ -n "$tb" ] && printf '  top blockers: %s\n' "$tb"
  fi

  # size:l is excluded from `ready` because `claim` refuses it. Say so, or the
  # queue looks empty while the work sits there.
  split="$(printf '%s' "$shaped" | jq -r "$JQ_LIB"'[.[] | select(needs_split) | .num]
           | map("#\(.)") | join(" ")')"
  [ -n "$split" ] && printf '  SPLIT: %s are size:l — break them up with add --parent.\n' "$split"
  trunc="$(printf '%s' "$shaped" | jq -r '[.[] | select(.trunc) | .num] | map("#\(.)") | join(" ")')"
  [ -n "$trunc" ] && printf '  TRUNCATED: %s have more relations than gh returns; treat as unknown.\n' "$trunc"
  [ -n "$cyc" ] && printf '  CYCLE: %s can never become ready. Run: scripts/track.sh graph\n' "$cyc"
  [ "$total" -ge "$LIST_LIMIT" ] && printf '  LIMIT: %s open issues fetched; raise TRACK_LIMIT, work may be hidden.\n' "$total"
  return 0
}

cmd_blocked() {
  local shaped payload total n tb
  shaped="$(fetch_open | jq "$JQ_LIB"' [.[] | shape]')"
  total="$(printf '%s' "$shaped" | jq 'length')"
  payload="$(printf '%s' "$shaped" | jq "$JQ_LIB"'
    [.[] | select(is_blocked)] | sort_by(.num)')"

  if [ "$AS_JSON" = 1 ]; then printf '%s\n' "$payload"; return 0; fi

  n="$(printf '%s' "$payload" | jq 'length')"
  printf 'blocked %s/%s open\n' "$n" "$total"
  printf '%s' "$payload" | jq -r --argjson m "$TITLE_MAX" '
    .[] | [ .num, (.size // ""), (.area // ""), (.kind // ""),
            (if (.title | length) > $m then (.title[0:$m] + "…") else .title end),
            (.blockers | map("#\(.)") | join(" "))
          ] | @tsv' \
  | awk -F'\t' '{
      printf "  #%-4s %-1s  %-7s %-6s %s  <- %s\n",
             $1, ($2 == "" ? "-" : $2), ($3 == "" ? "-" : $3), ($4 == "" ? "-" : $4), $5, $6;
    }'
  tb="$(printf '%s' "$shaped" | jq -r "$JQ_LIB"'
        top_blockers[0:5] | map("#\(.num) (\(.n))") | join("  ")')"
  [ -n "$tb" ] && printf 'top blockers: %s\n' "$tb"
  return 0
}

cmd_find() {
  [ $# -ge 1 ] || die "usage: track.sh find <term>"
  local term="$1" payload n total
  payload="$(fetch_all | jq "$JQ_LIB"' [.[] | shape]')"
  total="$(printf '%s' "$payload" | jq 'length')"
  payload="$(printf '%s' "$payload" | jq --arg t "$term" '
    [.[] | select(.title | ascii_downcase | contains($t | ascii_downcase))]
    | sort_by(.num) | reverse')"

  if [ "$AS_JSON" = 1 ]; then printf '%s\n' "$payload"; return 0; fi

  n="$(printf '%s' "$payload" | jq 'length')"
  printf 'find %s match(es) for "%s"\n' "$n" "$term"
  printf '%s' "$payload" | jq -r --argjson m "$TITLE_MAX" '
    .[] | [ .num, .state, (.size // ""), (.area // ""),
            (if (.title | length) > $m then (.title[0:$m] + "…") else .title end) ]
        | @tsv' \
  | awk -F'\t' '{ printf "  #%-4s %-6s %-1s  %-7s %s\n",
                  $1, tolower($2), ($3 == "" ? "-" : $3), ($4 == "" ? "-" : $4), $5 }'
  [ "$total" -ge "$LIST_LIMIT" ] && printf '  LIMIT: %s issues fetched; raise TRACK_LIMIT, matches may be hidden.\n' "$total"
  return 0
}

cmd_mine() {
  local me held n
  me="$(agent_id)"
  held="$(claimed_issues | jq -sc --arg me "$me" '[.[] | select(.claim.agent == $me)]')"

  if [ "$AS_JSON" = 1 ]; then printf '%s\n' "$held"; return 0; fi

  n="$(printf '%s' "$held" | jq 'length')"
  printf 'mine %s held by %s\n' "$n" "$me"
  printf '%s' "$held" | jq -r --argjson m "$TITLE_MAX" '
    .[] | [ .num, (.size // ""), (.area // ""), .claim.since,
            (if (.title | length) > $m then (.title[0:$m] + "…") else .title end) ]
        | @tsv' \
  | awk -F'\t' '{ printf "  #%-4s %-1s  %-7s %s  since %s\n",
                  $1, ($2 == "" ? "-" : $2), ($3 == "" ? "-" : $3), $5, $4 }'
  return 0
}

# The branch name doubles as the agent id, so it is built from the character set
# the claim marker parses with: anything else would produce a claim that can
# never be matched back.
slugify() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' \
    | sed -e 's/[^a-z0-9]\{1,\}/-/g' -e 's/^-//' -e 's/-$//' | cut -c1-40 | sed -e 's/-$//'
}

branch_for() {   # branch_for <kind> <num> <title>
  local kind="$1" num="$2" title="$3"
  printf '%s/%s-%s' "${kind:-task}" "$num" "$(slugify "$title")"
}

cmd_start() {
  [ $# -ge 1 ] || die "usage: track.sh start <n>"
  local n="${1#\#}" info kind title branch orig created=0 rc=0

  git rev-parse --git-dir >/dev/null 2>&1 || die "not inside a git repository."
  git diff --quiet && git diff --cached --quiet \
    || die "working tree has uncommitted changes. Finish or stash them first."
  git show-ref --verify --quiet refs/heads/main || die "no local 'main' to branch from."

  info="$(fetch_issue "$n" | jq -c "$JQ_LIB"' shape')"
  kind="$( printf '%s' "$info" | jq -r '.kind  // ""')"
  title="$(printf '%s' "$info" | jq -r '.title // ""')"
  [ -n "$title" ] || die "#$n has no title — is it a real issue?"
  branch="$(branch_for "$kind" "$n" "$title")"
  orig="$(git rev-parse --abbrev-ref HEAD)"

  if git show-ref --verify --quiet "refs/heads/$branch"; then
    git switch "$branch" >/dev/null 2>&1 || die "could not switch to existing branch $branch."
  else
    git switch -c "$branch" main >/dev/null 2>&1 || die "could not create branch $branch from main."
    created=1
  fi

  # cmd_claim exits on a fatal error, so it runs in a subshell: the branch has to
  # be rolled back here, and an exit would take that with it.
  rc=0
  ( cmd_claim "$n" ) || rc=$?
  if [ "$rc" -ne 0 ]; then
    if [ "$created" = 1 ]; then
      git switch "$orig" >/dev/null 2>&1 || true
      git branch -D "$branch" >/dev/null 2>&1 || true
      note "rolled back branch $branch — the claim did not succeed."
    else
      note "left existing branch $branch in place — the claim did not succeed."
    fi
    exit "$rc"
  fi
  printf 'started #%s on %s\n' "$n" "$branch"
  return 0
}

cmd_show() {
  [ $# -ge 1 ] || die "usage: track.sh show <n>"
  local payload
  payload="$(fetch_issue "${1#\#}" | jq "$JQ_LIB$JQ_CLAIM"'
    shape + {body: (.body // ""), claim: holder}')"

  if [ "$AS_JSON" = 1 ]; then printf '%s\n' "$payload"; return 0; fi

  printf '%s' "$payload" | jq -r '
    def line($k; $v): if ($v | length) > 0
                      then "\($k)\(" " * (8 - ($k | length)))\($v)" else empty end;
    "#\(.num)  \(.state)  area=\(if .area == "" then "-" else .area end) kind=\(if .kind == "" then "-" else .kind end) size=\(if .size == "" then "-" else .size end)",
    line("title"; .title),
    line("url";   .url),
    (if .claim != null then "claim   \(.claim.agent)  since \(.claim.since)" else empty end),
    (if .parent != null then "parent  #\(.parent.num) \(.parent.title)" else empty end),
    (if .subs.total > 0
       then "subs    \(.subs.completed)/\(.subs.total) done" +
            (if (.subs_open | length) > 0
               then ", open: " + (.subs_open | map("#\(.)") | join(" ")) else "" end)
       else empty end),
    ("needs   " +
      (if (.blockers | length) > 0 then (.blockers | map("#\(.)") | join(" "))
       else "(none open)" end) +
      (if (.done_deps | length) > 0
         then "   done: " + (.done_deps | map("#\(.)") | join(" ")) else "" end)),
    (if (.unblocks | length) > 0
       then "blocks  " + (.unblocks | map("#\(.)") | join(" ")) else empty end),
    "--- body",
    .body'
  return 0
}

cmd_claim() {
  local force=0 n="" me info state wip size blockers subs holder since rc=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --force) force=1; shift ;;
      -*)      die "unknown flag for claim: $1" ;;
      *)       n="${1#\#}"; shift ;;
    esac
  done
  [ -n "$n" ] || die "usage: track.sh claim <n> [--force]"
  me="$(agent_id)"

  lock_acquire                        # held across read+write => real CAS

  info="$(fetch_issue "$n" | jq -c "$JQ_LIB$JQ_CLAIM"' shape + {claim: holder}')"
  state="$(   printf '%s' "$info" | jq -r '.state')"
  wip="$(     printf '%s' "$info" | jq -r '.wip')"
  size="$(    printf '%s' "$info" | jq -r '.size // ""')"
  blockers="$(printf '%s' "$info" | jq -r '.blockers  | map("#\(.)") | join(" ")')"
  subs="$(    printf '%s' "$info" | jq -r '.subs_open | map("#\(.)") | join(" ")')"
  holder="$(  printf '%s' "$info" | jq -r '.claim.agent // ""')"

  [ "$state" = "OPEN" ] || { lock_release; die "#$n is $state — nothing to claim."; }
  [ -z "$blockers" ]    || { lock_release; die "#$n is blocked by $blockers. Work on a blocker instead."; }
  [ -z "$subs" ]        || { lock_release; die "#$n is a container (open sub-issues: $subs). Claim a sub-issue."; }

  if [ "$wip" = "true" ]; then
    if [ "$holder" = "$me" ]; then
      lock_release
      printf 'claimed #%s agent=%s (already yours)\n' "$n" "$me"
      return 0
    fi
    lock_release
    since="$(printf '%s' "$info" | jq -r '.claim.since // "unknown"')"
    note "#$n is claimed by ${holder:-unknown} since $since."
    note "If that claim is abandoned: scripts/track.sh release $n --force"
    printf 'busy #%s holder=%s\n' "$n" "${holder:-unknown}"
    return 2
  fi

  if [ "$size" = "l" ] && [ "$force" = 0 ]; then
    lock_release
    die "#$n is size:l — too big for one session. Split it:
  scripts/track.sh add -t '<part>' --parent $n --area <a> --kind <k> --size s
Then claim a sub-issue. Override with --force only if it really is one session."
  fi

  note "claiming #$n as $me …"
  rc=0
  gh_write issue edit "$n" --add-label wip >/dev/null || rc=$?
  [ "$rc" -eq 0 ] || { lock_release; die "could not label #$n — claim abandoned."; }
  # If the marker write fails the label is already on, so take it back off:
  # a wip label with no marker is a claim nobody can identify or release.
  rc=0
  gh_write issue comment "$n" --body "<!-- track:claim agent=$me -->
Claimed by \`$me\` via \`scripts/track.sh claim\`." >/dev/null || rc=$?
  if [ "$rc" -ne 0 ]; then
    gh_write issue edit "$n" --remove-label wip >/dev/null 2>&1 || true
    lock_release
    die "could not record the claim marker on #$n — claim rolled back."
  fi
  lock_release

  printf 'claimed #%s agent=%s\n' "$n" "$me"
  return 0
}

cmd_release() {
  local force=0 n="" me holder rc=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --force) force=1; shift ;;
      -*)      die "unknown flag for release: $1" ;;
      *)       n="${1#\#}"; shift ;;
    esac
  done
  [ -n "$n" ] || die "usage: track.sh release <n> [--force]"
  me="$(agent_id)"

  lock_acquire
  holder="$(fetch_issue "$n" | jq -r "$JQ_CLAIM"' holder.agent // ""')"
  if [ -n "$holder" ] && [ "$holder" != "$me" ] && [ "$force" = 0 ]; then
    lock_release
    die "#$n is held by $holder, not $me. Use --force to take it back."
  fi
  # Not `|| true`: a silently failed removal leaves wip set forever, which
  # drops the issue out of `ready` with nothing anywhere reporting why.
  rc=0
  gh_write issue edit "$n" --remove-label wip >/dev/null || rc=$?
  [ "$rc" -eq 0 ] || { lock_release; die "could not clear wip on #$n — it is still claimed."; }
  gh_write issue comment "$n" --body "<!-- track:release agent=$me -->
Released by \`$me\`." >/dev/null || true
  lock_release
  printf 'released #%s\n' "$n"
  return 0
}

cmd_done() {
  local force=0 n="" msg="" me info state subs holder was rc=0 freed
  while [ $# -gt 0 ]; do
    case "$1" in
      --force)      force=1; shift ;;
      -m|--message) [ $# -ge 2 ] || die "-m needs a message"; msg="$2"; shift 2 ;;
      -*)           die "unknown flag for done: $1" ;;
      *)            n="${1#\#}"; shift ;;
    esac
  done
  [ -n "$n" ] || die "usage: track.sh done <n> [-m MSG] [--force]"
  me="$(agent_id)"

  lock_acquire
  info="$(fetch_issue "$n" | jq -c "$JQ_LIB$JQ_CLAIM"' shape + {claim: holder}')"
  state="$( printf '%s' "$info" | jq -r '.state')"
  subs="$(  printf '%s' "$info" | jq -r '.subs_open | map("#\(.)") | join(" ")')"
  holder="$(printf '%s' "$info" | jq -r '.claim.agent // ""')"
  was="$(   printf '%s' "$info" | jq -r '.unblocks  | map("#\(.)") | join(" ")')"

  [ "$state" = "OPEN" ] || { lock_release; die "#$n is already $state."; }
  if [ -n "$subs" ] && [ "$force" = 0 ]; then
    lock_release; die "#$n still has open sub-issues: $subs. Close those first, or --force."
  fi
  if [ "$holder" != "$me" ] && [ "$force" = 0 ]; then
    lock_release
    die "#$n is held by ${holder:-nobody}, not $me. Claim it first, or --force."
  fi

  gh_write issue edit "$n" --remove-label wip >/dev/null 2>&1 || true
  gh_write issue comment "$n" --body "<!-- track:done agent=$me -->
${msg:-Completed by \`$me\`.}" >/dev/null || true
  rc=0
  gh_write issue close "$n" --reason completed >/dev/null || rc=$?
  lock_release
  [ "$rc" -eq 0 ] || die "could not close #$n — see the gh error above."

  # Report only what is genuinely actionable now. An issue this one was blocking
  # may still have other open blockers; naming it as unblocked sends the caller
  # into a claim that exits 1, which AGENTS.md tells agents to treat as fatal.
  freed=""
  if [ -n "$was" ]; then
    freed="$(fetch_open | jq -r "$JQ_LIB"'[.[] | shape | select(is_ready) | .num]
              | map("#\(.)") | join(" ")')"
    freed="$(printf '%s\n%s\n' "$was" "$freed" | tr ' ' '\n' | sort | uniq -d | tr '\n' ' ')"
    freed="${freed% }"
  fi
  if [ -n "$freed" ]; then
    printf 'done #%s  unblocked: %s\n' "$n" "$freed"
  else
    printf 'done #%s\n' "$n"
  fi
  return 0
}

add_flag_list() {   # $1 = flag, $2 = comma list; appends to GH_ARGS
  local flag="$1" list="$2" v
  [ -n "$list" ] || return 0
  local IFS=','
  for v in $list; do
    v="$(printf '%s' "$v" | tr -d '[:space:]')"
    v="${v#\#}"
    [ -n "$v" ] || continue
    case "$v" in *[!0-9]*) die "$flag: '$v' is not an issue number." ;; esac
    GH_ARGS[${#GH_ARGS[@]}]="$flag"
    GH_ARGS[${#GH_ARGS[@]}]="$v"
  done
  return 0
}

cmd_add() {
  local title="" body="" bodyfile="" area="" kind="" size=""
  local bby="" bing="" parent="" selftest=0 url num rc=0
  while [ $# -gt 0 ]; do
    case "$1" in
      -t|--title)     [ $# -ge 2 ] || die "-t needs a value"; title="$2"; shift 2 ;;
      -b|--body)      [ $# -ge 2 ] || die "-b needs a value"; body="$2"; shift 2 ;;
      -F|--body-file) [ $# -ge 2 ] || die "-F needs a path";  bodyfile="$2"; shift 2 ;;
      --area)         [ $# -ge 2 ] || die "--area needs a value"; area="$2"; shift 2 ;;
      --kind)         [ $# -ge 2 ] || die "--kind needs a value"; kind="$2"; shift 2 ;;
      --size)         [ $# -ge 2 ] || die "--size needs a value"; size="$2"; shift 2 ;;
      --blocked-by)   [ $# -ge 2 ] || die "--blocked-by needs a value"; bby="$2"; shift 2 ;;
      --blocking)     [ $# -ge 2 ] || die "--blocking needs a value"; bing="$2"; shift 2 ;;
      --parent)       [ $# -ge 2 ] || die "--parent needs a value"; parent="${2#\#}"; shift 2 ;;
      --selftest)     selftest=1; shift ;;
      *)              die "unknown flag for add: $1" ;;
    esac
  done
  [ -n "$title" ] || die "add requires -t/--title"
  [ -n "$area" ] && [ -n "$kind" ] && [ -n "$size" ] \
    || die "add requires --area, --kind and --size."
  valid_label "area:$area" || die "unknown area '$area'. Valid: $(label_values area)"
  valid_label "kind:$kind" || die "unknown kind '$kind'. Valid: $(label_values kind)"
  valid_label "size:$size" || die "unknown size '$size'. Valid: $(label_values size)"

  GH_ARGS=(issue create --title "$title"
           --label "area:$area" --label "kind:$kind" --label "size:$size")
  [ "$selftest" = 1 ] && GH_ARGS[${#GH_ARGS[@]}]="--label" && GH_ARGS[${#GH_ARGS[@]}]="track:selftest"
  if [ -n "$bodyfile" ]; then
    GH_ARGS[${#GH_ARGS[@]}]="--body-file"; GH_ARGS[${#GH_ARGS[@]}]="$bodyfile"
  else
    GH_ARGS[${#GH_ARGS[@]}]="--body"; GH_ARGS[${#GH_ARGS[@]}]="${body:-_No description._}"
  fi
  add_flag_list --blocked-by "$bby"
  add_flag_list --blocking   "$bing"
  if [ -n "$parent" ]; then
    GH_ARGS[${#GH_ARGS[@]}]="--parent"; GH_ARGS[${#GH_ARGS[@]}]="$parent"
  fi

  note "creating issue …"
  # Acquire in THIS shell, not inside the substitution below: `$( )` runs in a
  # subshell, so a lock_acquire in there sets LOCK_HELD only for the subshell and
  # our lock_release would leave the lock directory behind.
  lock_acquire
  rc=0
  url="$(gh_write "${GH_ARGS[@]}")" || rc=$?
  lock_release
  [ "$rc" -eq 0 ] || die "add failed — see the gh error above."
  num="${url##*/}"
  case "$num" in ''|*[!0-9]*) die "add: could not parse an issue number from '$url'";; esac
  if [ "$AS_JSON" = 1 ]; then
    jq -nc --arg n "$num" --arg u "$url" '{num: ($n | tonumber), url: $u}'
  else
    printf 'created #%s %s\n' "$num" "$url"
  fi
  return 0
}

cmd_dep() {
  [ $# -ge 1 ] || die "usage: track.sh dep <n> [--needs N] [--drop-needs N] [--child N] [--drop-child N]"
  local n="${1#\#}" desc="" rc=0
  shift
  GH_ARGS=(issue edit "$n")
  while [ $# -gt 0 ]; do
    case "$1" in
      --needs)      [ $# -ge 2 ] || die "--needs needs a value"
                    GH_ARGS[${#GH_ARGS[@]}]="--add-blocked-by";    GH_ARGS[${#GH_ARGS[@]}]="${2#\#}"
                    desc="$desc needs #${2#\#}"; shift 2 ;;
      --drop-needs) [ $# -ge 2 ] || die "--drop-needs needs a value"
                    GH_ARGS[${#GH_ARGS[@]}]="--remove-blocked-by"; GH_ARGS[${#GH_ARGS[@]}]="${2#\#}"
                    desc="$desc drop-needs #${2#\#}"; shift 2 ;;
      --child)      [ $# -ge 2 ] || die "--child needs a value"
                    GH_ARGS[${#GH_ARGS[@]}]="--add-sub-issue";     GH_ARGS[${#GH_ARGS[@]}]="${2#\#}"
                    desc="$desc child #${2#\#}"; shift 2 ;;
      --drop-child) [ $# -ge 2 ] || die "--drop-child needs a value"
                    GH_ARGS[${#GH_ARGS[@]}]="--remove-sub-issue";  GH_ARGS[${#GH_ARGS[@]}]="${2#\#}"
                    desc="$desc drop-child #${2#\#}"; shift 2 ;;
      *)            die "unknown flag for dep: $1" ;;
    esac
  done
  [ "${#GH_ARGS[@]}" -gt 3 ] || die "dep needs at least one of --needs/--drop-needs/--child/--drop-child"
  # Explicit status check: when this function is called as `cmd_dep … || rc=$?`,
  # bash disables `set -e` for the whole body, so a failed write would otherwise
  # fall through to the success message below. GitHub rejects a direct 2-cycle
  # here, and that rejection must reach the caller.
  rc=0
  gh_write "${GH_ARGS[@]}" >/dev/null || rc=$?
  lock_release
  [ "$rc" -eq 0 ] || die "dep failed on #$n — see the gh error above."
  printf 'dep #%s%s\n' "$n" "$desc"
  return 0
}

cmd_note() {
  [ $# -ge 1 ] || die "usage: track.sh note <n> -m MSG"
  local n="${1#\#}" msg="" rc=0
  shift
  while [ $# -gt 0 ]; do
    case "$1" in
      -m|--message) [ $# -ge 2 ] || die "-m needs a message"; msg="$2"; shift 2 ;;
      *)            die "unknown flag for note: $1" ;;
    esac
  done
  [ -n "$msg" ] || die "note requires -m MSG"
  # A comment body containing a track: marker would be parsed as claim state.
  case "$msg" in *'<!-- track:'*) die "note body may not contain a '<!-- track:' marker." ;; esac
  rc=0
  gh_write issue comment "$n" --body "$msg" >/dev/null || rc=$?
  lock_release
  [ "$rc" -eq 0 ] || die "note failed on #$n — see the gh error above."
  printf 'noted #%s\n' "$n"
  return 0
}

cmd_graph() {
  local shaped cyc
  shaped="$(fetch_open | jq "$JQ_LIB"' [.[] | shape]')"
  if [ "$AS_JSON" = 1 ]; then printf '%s\n' "$shaped"; return 0; fi

  printf 'graph (%s open)\n' "$(printf '%s' "$shaped" | jq 'length')"
  cyc="$(printf '%s' "$shaped" | jq -r "$JQ_LIB"'cycle_nodes | map("#\(.)") | join(" ")')"
  printf '%s' "$shaped" | jq -r --argjson m 60 '
    ([.[] | {key: (.num | tostring), value: .}] | from_entries) as $idx
    | def pad($d): if $d == 0 then "" else ("  " * $d) end;
      def walk($n; $d):
        ($idx[$n | tostring]) as $i
        | if $i == null then empty
          elif $d > 5 then "\(pad($d))#\($n) …"
          else "\(pad($d))#\($n) \(if $i.area == "" then "-" else $i.area end) \(
                 if ($i.title | length) > $m then ($i.title[0:$m] + "…") else $i.title end
               )\(if $i.wip then "  [wip]" else "" end)",
               ($i.unblocks[]? | walk(.; $d + 1))
          end;
      [.[] | select(.blockers | length == 0) | .num] as $roots
      | ($roots[] | walk(.; 0))'
  [ -n "$cyc" ] && printf 'CYCLE: %s — unreachable from any root, they block each other.\n' "$cyc"
  return 0
}

cmd_labels_init() {
  local name color desc total
  total="$(label_names | wc -l | tr -d ' ')"
  note "creating/updating $total labels (paced) …"
  while IFS='|' read -r name color desc; do
    [ -n "$name" ] || continue
    gh_write label create "$name" --color "$color" --description "$desc" --force >/dev/null
    note "  $name"
  done <<< "$LABEL_SPEC"
  lock_release
  printf 'labels-init ok (%s labels)\n' "$total"
  return 0
}

# ----------------------------------------------------------------- doctor ---
DOC_FAIL=0
DOC_JSON='[]'
chk() {   # $1 = ok|FAIL|warn, $2 = name, $3 = detail
  DOC_JSON="$(printf '%s' "$DOC_JSON" | jq -c --arg s "$1" --arg n "$2" --arg d "$3" \
              '. + [{check: $n, status: $s, detail: $d}]')"
  [ "$1" = "FAIL" ] && DOC_FAIL=$((DOC_FAIL + 1))
  [ "$AS_JSON" = 1 ] || printf '  %-5s %-22s %s\n' "$1" "$2" "$3"
  return 0
}

ver_ge() {   # ver_ge 2.97.0 2.94.0  — `sort -V` is not reliable on BSD
  local have="$1" want="$2" h1 h2 h3 w1 w2 w3
  h1="${have%%.*}"; have="${have#*.}"; h2="${have%%.*}"; h3="${have#*.}"; h3="${h3%%[!0-9]*}"
  w1="${want%%.*}"; want="${want#*.}"; w2="${want%%.*}"; w3="${want#*.}"; w3="${w3%%[!0-9]*}"
  [ "${h1:-0}" -gt "${w1:-0}" ] && return 0
  [ "${h1:-0}" -lt "${w1:-0}" ] && return 1
  [ "${h2:-0}" -gt "${w2:-0}" ] && return 0
  [ "${h2:-0}" -lt "${w2:-0}" ] && return 1
  [ "${h3:-0}" -ge "${w3:-0}" ]
}

cmd_doctor() {
  local ghv st who scopes repo nwo issues have missing L me cyc total
  local stale c num who2 since at age
  [ "$AS_JSON" = 1 ] || printf 'doctor\n'

  if command -v jq >/dev/null 2>&1; then chk ok jq "$(jq --version)"
  else chk FAIL jq "not installed — brew install jq"; fi

  if command -v gh >/dev/null 2>&1; then
    ghv="$(gh --version | awk 'NR==1{print $3}')"
    if ver_ge "$ghv" "2.94.0"; then chk ok gh "$ghv (>= 2.94)"
    else chk FAIL gh "$ghv — need >= 2.94 for --blocked-by/--parent. brew upgrade gh"; fi
  else chk FAIL gh "not installed"; fi

  st="$(gh auth status 2>&1 || true)"
  if printf '%s' "$st" | grep -q 'Logged in'; then
    who="$(gh api user --jq .login 2>/dev/null || echo '?')"
    scopes="$(printf '%s' "$st" | grep -o "Token scopes:.*" | awk 'NR==1')"
    if printf '%s' "$scopes" | grep -q "'repo'"; then chk ok auth "$who; $scopes"
    else chk FAIL auth "$who has no 'repo' scope — gh auth refresh -s repo"; fi
  else chk FAIL auth "not logged in — gh auth login"; fi

  repo="$(gh repo view --json nameWithOwner,hasIssuesEnabled 2>/dev/null || true)"
  if [ -n "$repo" ]; then
    nwo="$(   printf '%s' "$repo" | jq -r .nameWithOwner)"
    issues="$(printf '%s' "$repo" | jq -r .hasIssuesEnabled)"
    if [ "$issues" = "true" ]; then chk ok repo "$nwo (issues enabled)"
    else chk FAIL repo "$nwo has Issues disabled — enable in repo settings"; fi
  else chk FAIL repo "cannot resolve repo from git remote"; fi

  if gh issue list --limit 1 --json number,blockedBy,subIssues >/dev/null 2>&1; then
    chk ok dep-api "blockedBy/subIssues readable"
  else chk FAIL dep-api "cannot read dependency fields"; fi

  total="$(label_names | wc -l | tr -d ' ')"
  have="$(gh label list --limit 200 --json name --jq '.[].name' 2>/dev/null || true)"
  missing=""
  while IFS= read -r L; do
    [ -n "$L" ] || continue
    printf '%s\n' "$have" | grep -qx -- "$L" || missing="$missing $L"
  done <<< "$(label_names)"
  if [ -z "$missing" ]; then chk ok labels "all $total present"
  else chk FAIL labels "missing:$missing — run: scripts/track.sh labels-init"; fi

  me="$(agent_id 2>/dev/null || true)"
  if [ -n "$me" ]; then chk ok agent "$me"
  else chk warn agent "on main/detached — claim will refuse. Branch, or set MOTIF_AGENT."; fi

  if repo_key >/dev/null 2>&1; then
    state_init
    if mkdir -p "$STATE_DIR" 2>/dev/null && [ -w "$STATE_DIR" ]; then
      chk ok lockdir "$STATE_DIR"
    else chk FAIL lockdir "$STATE_DIR not writable"; fi
  else chk FAIL lockdir "not inside a git repository"; fi

  cyc="$(fetch_open | jq "$JQ_LIB"'[.[] | shape] | has_cycle' 2>/dev/null || echo false)"
  if [ "$cyc" = "true" ]; then chk FAIL graph "dependency cycle — run: scripts/track.sh graph"
  else chk ok graph "no dependency cycle"; fi

  # Branches here live hours, so a claim older than a day is a strong signal.
  # It is a warning and never a failure: a slow task and a dead one look
  # identical from here, and only a human can tell them apart.
  stale=""
  while IFS= read -r c; do
    [ -n "$c" ] || continue
    num="$(  printf '%s' "$c" | jq -r '.num')"
    who2="$( printf '%s' "$c" | jq -r '.claim.agent // ""')"
    since="$(printf '%s' "$c" | jq -r '.claim.since // ""')"
    [ -n "$since" ] || continue
    at="$(iso_epoch "$since")"
    [ "$at" -gt 0 ] || continue
    age=$(( ( $(date -u +%s) - at ) / 3600 ))
    [ "$age" -ge "$STALE_HOURS" ] && stale="$stale #$num($who2, ${age}h)"
  done <<< "$(claimed_issues 2>/dev/null || true)"
  if [ -n "$stale" ]; then
    chk warn claims "stale:$stale — release with: scripts/track.sh release <n> --force"
  else chk ok claims "no claim older than ${STALE_HOURS}h"; fi

  if [ "$AS_JSON" = 1 ]; then
    printf '%s' "$DOC_JSON" | jq -c --argjson f "$DOC_FAIL" '{checks: ., failed: $f}'
  elif [ "$DOC_FAIL" -eq 0 ]; then printf 'doctor ok\n'
  else printf '%s check(s) failed\n' "$DOC_FAIL"; fi
  [ "$DOC_FAIL" -eq 0 ] || exit 1
  return 0
}

# --------------------------------------------------------------- selftest ---
ST_PASS=0
ST_FAIL=0
st_ok()   { ST_PASS=$((ST_PASS + 1)); note "  ok    $*"; return 0; }
st_bad()  { ST_FAIL=$((ST_FAIL + 1)); note "  FAIL  $*"; return 0; }
st_assert() { if [ "$1" = 0 ]; then st_ok "$2"; else st_bad "$2"; fi; }

st_cleanup() {
  local nums n deleted=0
  note "  cleaning up …"
  nums="$(gh issue list --state all --label track:selftest --limit 100 \
          --json number --jq '.[].number' 2>/dev/null || true)"
  for n in $nums; do
    if gh_write issue delete "$n" --yes >/dev/null 2>&1; then
      deleted=$((deleted + 1))
    else
      gh_write issue close "$n" --reason "not planned" >/dev/null 2>&1 || true
      note "  (could not delete #$n — closed instead; needs admin to delete)"
    fi
  done
  lock_release
  note "  cleanup: removed $deleted throwaway issue(s)"
  return 0
}

st_num() { printf '%s' "$1" | awk '{print $2}' | tr -d '#'; }

cmd_selftest() {
  [ "${1:-}" = "--yes" ] || die "selftest creates and deletes real issues in this repo.
Re-run with:  scripts/track.sh selftest --yes"

  local t0 A B C D E F G H I J out rc loc adv dt
  t0="$(date +%s)"
  note "selftest"
  note "  preflight: doctor"
  ( AS_JSON=0 cmd_doctor >/dev/null ) || die "doctor failed — fix that first."

  trap 'st_cleanup; lock_release' EXIT
  # Without an explicit exit, bash runs the handler and then RESUMES, so a Ctrl-C
  # would delete the throwaway issues and carry on asserting against them.
  trap 'st_cleanup; lock_release; exit 130' INT TERM

  note "  creating throwaway issues …"
  A="$(st_num "$(AS_JSON=0 cmd_add -t "selftest parent" --area infra --kind chore --size s --selftest)")"
  C="$(st_num "$(AS_JSON=0 cmd_add -t "selftest child of $A" --area infra --kind chore --size s --parent "$A" --selftest)")"
  B="$(st_num "$(AS_JSON=0 cmd_add -t "selftest blocked by $A" --area infra --kind chore --size s --blocked-by "$A" --selftest)")"
  st_ok "created #$A (parent) #$C (child) #$B (blocked by #$A)"

  out="$(AS_JSON=1 cmd_ready)"
  rc=0; printf '%s' "$out" | jq -e --argjson n "$C" 'any(.num == $n)' >/dev/null || rc=1
  st_assert "$rc" "ready includes leaf #$C"
  rc=0; printf '%s' "$out" | jq -e --argjson n "$A" 'all(.num != $n)' >/dev/null || rc=1
  st_assert "$rc" "ready excludes container #$A"
  rc=0; printf '%s' "$out" | jq -e --argjson n "$B" 'all(.num != $n)' >/dev/null || rc=1
  st_assert "$rc" "ready excludes blocked #$B"

  rc=0; AS_JSON=1 cmd_blocked | jq -e --argjson n "$B" --argjson a "$A" \
      'any(.num == $n and (.blockers | index($a) != null))' >/dev/null || rc=1
  st_assert "$rc" "blocked lists #$B <- #$A"

  rc=0; MOTIF_AGENT=selftest-1 cmd_claim "$C" >/dev/null || rc=$?
  st_assert "$rc" "claim #$C as selftest-1"

  rc=0; MOTIF_AGENT=selftest-2 cmd_claim "$C" >/dev/null 2>&1 || rc=$?
  st_assert "$([ "$rc" = 2 ] && echo 0 || echo 1)" "second claim rejected with exit 2 (got $rc)"

  rc=0; MOTIF_AGENT=selftest-1 cmd_claim "$C" >/dev/null 2>&1 || rc=$?
  st_assert "$([ "$rc" = 0 ] && echo 0 || echo 1)" "re-claim by owner is idempotent"

  rc=0; AS_JSON=1 cmd_ready | jq -e --argjson n "$C" 'all(.num != $n)' >/dev/null || rc=1
  st_assert "$rc" "ready excludes claimed #$C"

  MOTIF_AGENT=selftest-1 cmd_release "$C" >/dev/null
  rc=0; AS_JSON=1 cmd_ready | jq -e --argjson n "$C" 'any(.num == $n)' >/dev/null || rc=1
  st_assert "$rc" "release returns #$C to ready"

  MOTIF_AGENT=selftest-1 cmd_claim "$C" >/dev/null
  MOTIF_AGENT=selftest-1 cmd_done  "$C" >/dev/null
  rc=0; AS_JSON=1 cmd_show "$C" | jq -e '.state == "CLOSED" and (.wip | not)' >/dev/null || rc=1
  st_assert "$rc" "done closes #$C and clears wip"

  rc=0; AS_JSON=1 cmd_ready | jq -e --argjson n "$A" 'any(.num == $n)' >/dev/null || rc=1
  st_assert "$rc" "#$A leaves container state once #$C closes"

  out="$(MOTIF_AGENT=selftest-1 cmd_done "$A" --force)"
  rc=0; printf '%s' "$out" | grep -q "unblocked: #$B" || rc=1
  st_assert "$rc" "done #$A reports 'unblocked: #$B'"

  rc=0; AS_JSON=1 cmd_ready | jq -e --argjson n "$B" 'any(.num == $n)' >/dev/null || rc=1
  st_assert "$rc" "#$B becomes ready when its blocker closes"

  # An issue with two blockers must not be announced when only one closes:
  # the caller would claim it and get a fatal exit 1.
  G="$(st_num "$(AS_JSON=0 cmd_add -t "selftest gate G" --area infra --kind chore --size s --selftest)")"
  H="$(st_num "$(AS_JSON=0 cmd_add -t "selftest gate H" --area infra --kind chore --size s --selftest)")"
  I="$(st_num "$(AS_JSON=0 cmd_add -t "selftest needs G and H" --area infra --kind chore --size s --blocked-by "$G,$H" --selftest)")"
  MOTIF_AGENT=selftest-1 cmd_claim "$G" >/dev/null
  out="$(MOTIF_AGENT=selftest-1 cmd_done "$G")"
  rc=0; printf '%s' "$out" | grep -q "unblocked:" && rc=1
  st_assert "$rc" "done #$G stays quiet: #$I still needs #$H"
  MOTIF_AGENT=selftest-1 cmd_claim "$H" >/dev/null
  out="$(MOTIF_AGENT=selftest-1 cmd_done "$H")"
  rc=0; printf '%s' "$out" | grep -q "unblocked: #$I" || rc=1
  st_assert "$rc" "done #$H reports #$I once its last blocker closes"

  # size:l is refused by claim, so it must not be offered by ready.
  J="$(st_num "$(AS_JSON=0 cmd_add -t "selftest oversized" --area infra --kind chore --size l --selftest)")"
  rc=0; AS_JSON=1 cmd_ready | jq -e --argjson n "$J" 'all(.num != $n)' >/dev/null || rc=1
  st_assert "$rc" "ready excludes size:l #$J"
  out="$(AS_JSON=0 cmd_ready)"
  rc=0; printf '%s' "$out" | grep -q "SPLIT:.*#$J" || rc=1
  st_assert "$rc" "ready reports #$J under SPLIT rather than hiding it"
  rc=0; ( MOTIF_AGENT=selftest-1 cmd_claim "$J" ) >/dev/null 2>&1 || rc=$?
  st_assert "$([ "$rc" = 1 ] && echo 0 || echo 1)" "claim refuses size:l #$J (got $rc)"

  # A whitespace agent id would produce an unmatchable claim marker.
  rc=0; ( MOTIF_AGENT="bad id" cmd_claim "$J" ) >/dev/null 2>&1 || rc=$?
  st_assert "$([ "$rc" = 1 ] && echo 0 || echo 1)" "claim rejects a whitespace agent id (got $rc)"

  # GitHub rejects a direct 2-cycle server-side but does NOT check transitively,
  # so a 3-cycle is reachable and is what we must detect. Verified 2026-08-03.
  D="$(st_num "$(AS_JSON=0 cmd_add -t "selftest cycle D" --area infra --kind chore --size s --selftest)")"
  E="$(st_num "$(AS_JSON=0 cmd_add -t "selftest cycle E" --area infra --kind chore --size s --blocked-by "$D" --selftest)")"
  F="$(st_num "$(AS_JSON=0 cmd_add -t "selftest cycle F" --area infra --kind chore --size s --blocked-by "$E" --selftest)")"

  rc=0; cmd_dep "$D" --needs "$F" >/dev/null 2>&1 || rc=$?
  st_assert "$rc" "closed the 3-cycle #$D <- #$F <- #$E <- #$D"

  # The cycle must be found even though unrelated work (#B) is still ready —
  # exactly the case a naive "no source anywhere" check misses.
  # NOTE: the library and the expression must be ONE argument. Passing them as
  # two makes jq treat the second as an input filename.
  rc=0
  AS_JSON=1 cmd_graph \
    | jq -e --argjson d "$D" --argjson e "$E" --argjson f "$F" \
        "$JQ_LIB"'cycle_nodes as $c
         | ($c | index($d) != null) and ($c | index($e) != null) and ($c | index($f) != null)' \
        >/dev/null 2>&1 || rc=1
  st_assert "$rc" "cycle #$D/#$E/#$F detected while #$B is still ready"

  # Capture first: piping into `grep -q` closes the pipe early and SIGPIPEs the
  # producer under `set -o pipefail`.
  out="$(AS_JSON=0 cmd_ready)"
  rc=0; printf '%s' "$out" | grep -q "CYCLE:.*#$D" || rc=1
  st_assert "$rc" "ready reports the cycle on stdout"

  rc=0; printf '%s' "$out" | grep -q "^  #$B " || rc=1
  st_assert "$rc" "#$B still listed as ready despite the cycle"

  # A direct 2-cycle is refused by the server; the wrapper must surface that.
  # Run in a subshell — `die` calls `exit`, which would otherwise end the run.
  rc=0; ( cmd_dep "$E" --needs "$F" ) >/dev/null 2>&1 || rc=$?
  st_assert "$([ "$rc" != 0 ] && echo 0 || echo 1)" "server rejects a direct 2-cycle, wrapper exits non-zero (got $rc)"

  # Informational canary: the search index is expected to disagree (it lags writes).
  # This documents WHY local derivation is the primary path. Never fails the run.
  loc="$(AS_JSON=1 cmd_ready | jq -r '[.[].num] | sort | join(",")')"
  adv="$(gh issue list --search 'is:open -is:blocked' --limit 100 --json number \
         --jq '[.[].number] | sort | join(",")' 2>/dev/null || echo 'n/a')"
  if [ "$adv" != "$loc" ]; then
    note "  note  advanced-search ready set differs from locally-derived set"
    note "        local:  $loc"
    note "        search: $adv"
    note "        (expected — the search index lags writes. Local derivation wins.)"
  else
    note "  note  advanced-search agrees with local derivation"
  fi

  trap - EXIT INT TERM
  st_cleanup
  dt=$(( $(date +%s) - t0 ))
  if [ "$ST_FAIL" -eq 0 ]; then
    printf 'selftest passed %s/%s in %ss\n' "$ST_PASS" "$((ST_PASS + ST_FAIL))" "$dt"
  else
    printf 'selftest FAILED %s/%s in %ss\n' "$ST_FAIL" "$((ST_PASS + ST_FAIL))" "$dt"
    exit 1
  fi
  return 0
}

# --------------------------------------------------------------- dispatch ---
AS_JSON=0
ARGS=()
for a in "$@"; do
  if [ "$a" = "--json" ]; then AS_JSON=1; else ARGS[${#ARGS[@]}]="$a"; fi
done
set -- ${ARGS[@]+"${ARGS[@]}"}
[ $# -gt 0 ] || usage
CMD="$1"; shift

case "$CMD" in
  ready)        cmd_ready "$@" ;;
  blocked)      cmd_blocked "$@" ;;
  find)         cmd_find "$@" ;;
  show)         cmd_show "$@" ;;
  start)        cmd_start "$@" ;;
  claim)        cmd_claim "$@" ;;
  mine)         cmd_mine "$@" ;;
  release)      cmd_release "$@" ;;
  done)         cmd_done "$@" ;;
  add)          cmd_add "$@" ;;
  dep)          cmd_dep "$@" ;;
  note)         cmd_note "$@" ;;
  graph)        cmd_graph "$@" ;;
  labels-init)  cmd_labels_init "$@" ;;
  doctor)       cmd_doctor "$@" ;;
  selftest)     cmd_selftest "$@" ;;
  -h|--help)    usage ;;
  *)            note "unknown command: $CMD"; usage ;;
esac
