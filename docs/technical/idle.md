# IMAP IDLE — mail in seconds (RFC 2177)

Spec: `specs/idle/spec.md`. How new mail arrives within seconds instead of on the 5-minute poll.

## A trigger layered on the scheduler

`geleit_engine::imap::idle_watch(config, secrets, folder, on_activity)` holds one connection: it checks
the server advertises `IDLE` (else `ImapError::IdleUnsupported`), `SELECT`s the folder, and loops —
`IDLE`, wait, and on a server push calls `on_activity`, on a re-IDLE tick close and re-`IDLE` on the same
connection. It returns only on a connection error (so the caller reconnects) or when IDLE is unsupported
(so the caller stops).

**Timeouts everywhere.** Every command (`CAPABILITY`, `SELECT`, `IDLE`/`DONE`) has a 60-second ceiling,
and the wait is re-IDLEd on a **wall clock** (28 min, under RFC 2177's 29) — an outer `tokio` timer, not
the library's `wait_with_timeout`, because that one resets on every server keepalive (`* OK Still here`)
and so would never elapse on a chatty server. Without these, a **half-open** connection (a slept laptop,
a NAT that dropped the flow) would hang a read forever and the watcher would never reconnect; with them
it errors out and the app's backoff loop reconnects.

`on_activity` is deliberately tiny. In the app (`idle.rs`), it's `wake.notify_waiters()` — the **exact
same poke** a successful Refresh uses (`AppState::wake_sync`). So an IDLE push and a Refresh drive the
identical scheduler sweep, which does the real fetch / notify / badge / re-list. IDLE never touches the
store or the sync itself; it is a latency shortcut, not a second code path. That's what keeps it from
drifting from the polling path — there's only one path.

`on_activity` uses `notify_one`, not `notify_waiters`: a push that lands while the scheduler is
*mid-sweep* would be lost by `notify_waiters` (it wakes only parked waiters) and the mail would wait out
the whole poll interval — exactly the latency IDLE exists to remove. `notify_one` stores a permit, so
the in-progress sweep is followed immediately by another.

`idle.rs` spawns one watcher task per account at boot (after a 5s delay so it doesn't fight the boot
sync), reconnecting with a gentle capped backoff (10s → 5min) when the connection drops. A connection
that lasted a good while before dropping was healthy, so backoff resets and it reconnects promptly; one
that fails fast keeps backing off. A drop is ordinary and never surfaced; the poll covers the gap. An
account whose config has vanished (removed) ends its task.

## Why the poll stays

IDLE is a shortcut, not a replacement. The 5-minute scheduler remains for: providers without IDLE; the
windows between an IDLE drop and its reconnect; folders other than the INBOX; and an account added after
launch (which gets IDLE only on the next start — a named limitation, harmless because the poll delivers
its mail meanwhile). Because both drive the same sweep, running both costs at most a redundant sweep,
which the per-folder sync lock and backoff already absorb.

## Verified live

`idle_wakes_within_seconds_when_mail_arrives` (Dovecot, `--features dangerous-tls`): watch a folder,
deliver a message from a *second* connection, and `on_activity` fires in well under a second — the test
bounds it at 10s. End to end, launching the app against Dovecot with the **default 5-minute poll**, a
message delivered from outside bumped the unread badge ~4 seconds later, ~16 seconds after boot — before
the first scheduled sweep (30s), so only IDLE could have driven it.
