# Resource campaign outcome

Source `abeb7bcf2ee653bff899ba14f505ced9b373f963`. The controller attempts a 30-second preview, then an actual 86,400-second campaign only if the preview runner and validator pass. The build profile is explicit and checked against Cargo artifact metadata before running. Deadlines and resource limits are unchanged. Original reports and failures are retained.

- preview: build profile release; actual 31.25390351099486 seconds; resource checks passed; 24-hour disposition not_run.
- 24h: build profile release; actual 7583.518504329004 seconds; resource checks failed; 24-hour disposition failed.

Overall production qualification remains false. This resource fixture does not close performance, feature, allocator high-water, kernel-memory, or dedicated-host release gates. See outcome.json for runner and validator exit codes, and each capsule manifest for raw hashes. Run the capsule check.py to verify its exact inventory.
