# Resource campaign outcome

Source `684847461083d1a7d680031f02b05fabb17ab59b`. The controller attempts a 30-second preview, then an actual 86,400-second campaign only if the preview runner and validator pass. The build profile is explicit and checked against Cargo artifact metadata before running. Deadlines and resource limits are unchanged. Original reports and failures are retained.

- preview: build profile release; actual 31.3500749800005 seconds; resource checks passed; 24-hour disposition not_run.
- 24h: build profile release; actual 1869.507058942003 seconds; resource checks failed; 24-hour disposition failed.

Overall production qualification remains false. This resource fixture does not close performance, feature, allocator high-water, kernel-memory, or dedicated-host release gates. See outcome.json for runner and validator exit codes, and each capsule manifest for raw hashes. Run the capsule check.py to verify its exact inventory.
