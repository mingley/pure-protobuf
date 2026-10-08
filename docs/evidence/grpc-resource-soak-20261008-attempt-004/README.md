# Resource campaign running

Source `50ac70e5c9818fe870719b37acede99f12710fe9`. The preview passed 8 cycles in 34.50 seconds; independent validation found no failures. It covered plaintext/TLS and identity/gzip with unchanged limits.

The actual 86,400-second child started at 2026-10-08 22:23:33 UTC. Its earliest requested finish is 2026-10-09 22:23:33 UTC. It was alive when launch.json was captured, with address space 1 GiB, 128 file descriptors, 4,096 same-uid processes/threads, and 86,430 CPU seconds as both soft and hard limits. This is a launch observation, not a completion report.

The source checkout and copied executable are frozen. The controller retains failures and is configured to publish the final outcome directly to main. See started.json for command and controller hashes, and launch.json for the preview validation, child hash, effective limits, and observed times. Overall production qualification remains false.
