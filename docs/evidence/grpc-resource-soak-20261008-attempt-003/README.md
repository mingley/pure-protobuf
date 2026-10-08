# Resource campaign outcome

Source `392c6772bf867eb5e0cda7805f8d2431d1905b1a`. The controller ran a 30-second preview before requesting an actual 86,400-second campaign. Original reports and failures are retained.

- preview: actual 18.403103144999477 seconds; resource checks failed; 24-hour disposition not_run.

Overall production qualification remains false. This resource fixture does not close performance, feature, allocator high-water, kernel-memory, or dedicated-host release gates. See outcome.json for runner and validator exit codes, and each capsule manifest for raw hashes. Run the capsule check.py to verify its exact inventory.
