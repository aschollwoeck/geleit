# Save & Open mail · Tasks
- [x] engine: export_eml(header, body) -> Result<Vec<u8>,String> (mail-builder) + round-trip test
- [x] engine: ImportedEml + parse_eml(raw) (mail-parser + mime::parse_body) + garbage test
- [x] store: SAVED_FOLDER + is_local_folder; prune_folders keeps local "Saved" + test
- [x] app: pick_save_path(default_name) + safe_filename_stem (+ keep pick_file_via_dialog for open)
- [x] app: "Save" action + save-message handler (export → save dialog → write; status feedback)
- [x] app: "Open mail file…" rail entry + open-mail-file handler (pick → parse → import to Saved →
      reload → switch folder → open)
- [x] verify in-app (GELEIT_SHOT=openeml + GELEIT_PICK_FILE): imported .eml renders with decoded
      subject in a new "Saved" folder; gates green
- [ ] PR merged
