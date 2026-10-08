# Resource campaign outcome

Source `50ac70e5c9818fe870719b37acede99f12710fe9`. The controller attempts a 30-second preview, then an actual 86,400-second campaign only if the preview runner and validator pass. Original reports and failures are retained.

- preview: actual 34.49582712100164 seconds; resource checks passed; 24-hour disposition not_run.
- 24h: actual 705.1768727369999 seconds; resource checks failed; 24-hour disposition failed.

Overall production qualification remains false. This resource fixture does not close performance, feature, allocator high-water, kernel-memory, or dedicated-host release gates. See outcome.json for runner and validator exit codes, and each capsule manifest for raw hashes. Run the capsule check.py to verify its exact inventory.
