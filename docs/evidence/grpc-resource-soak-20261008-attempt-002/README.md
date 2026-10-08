# Resource campaign outcome

Source `d659c2ff9a9186ad76da092ea407ad784ba07fe9`. The controller ran a 30-second preview before requesting an actual 86,400-second campaign. Original reports and failures are retained.

- preview: actual 32.880406381000284 seconds; resource checks failed; 24-hour disposition not_run.

Overall production qualification remains false. This resource fixture does not close performance, feature, allocator high-water, kernel-memory, or dedicated-host release gates. See outcome.json for runner and validator exit codes, and each capsule manifest for raw hashes. Run the capsule check.py to verify its exact inventory.
