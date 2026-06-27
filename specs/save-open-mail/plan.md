# Plan — Save & Open mail (.eml)

1. **Engine `export_eml`** — mirror `build()`’s MessageBuilder use; map MessageHeader→headers,
   StoredBody→text/html parts; split `to_addrs`/`cc_addrs` (comma-joined) via the `addresses` helper;
   set Date from the stored unix-seconds and Message-ID if present. Unit test: build → parse_body
   recovers plain+html.
2. **Engine `parse_eml` + `ImportedEml`** — `MessageParser` for subject/from(name,addr)/to/date/
   message-id (RFC2047-decoded); reuse `mime::parse_body` for plain/html/attachments flag. Unit test:
   a small raw message parses to the expected fields; round-trip with `export_eml`.
3. **Store prune protection** — `const LOCAL_FOLDERS = ["Saved"]`; `prune_folders` keeps any folder
   whose name matches (case-insensitive) regardless of the server `keep` list. Unit test.
4. **App picker** — generalise `pick_file_via_dialog` into open + `pick_save_path(default_name)`
   (zenity `--file-selection --save --filename=… --confirm-overwrite`; kdialog `--getsavefilename`).
5. **App Save** — reading-pane "Save" link → `save-message()` → `header_by_id`+`body_for` →
   `export_eml` → `pick_save_path(subject.eml)` → write; status on success/failure.
6. **App Open** — rail "Open mail file…" → `open-mail-file()` → `pick_file_via_dialog` → read bytes →
   `parse_eml` → upsert "Saved" folder + `upsert_message`(uid None) + `store_body` → reload → select
   the new message. Guard: needs an account.
7. Gates + a screenshot of an opened `.eml` rendering.
