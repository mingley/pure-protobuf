# Publication controls

Two synthetic checks run the exact campaign controller against disposable local Git repositories. A deliberately failed builder is retained and published to the local main branch. A mismatched preparation source is rejected and leaves that branch unchanged. No live remote, actual build, or performance measurement is involved. The capsule retains the controller hash, commands, and both outcomes. Run `python3 check.py` to verify its inventory.
