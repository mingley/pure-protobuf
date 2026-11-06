#!/usr/bin/env python3
"""Select an exact release source and mode without registry access."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys


CI_PATH = ".github/workflows/ci.yml"
SCHEMA = "pbrs-release-context/1"


class ContextError(ValueError):
    """An explicit release request does not satisfy the existing policy."""


def select_context(
    *, event_name, payload, repository, repository_id, source_sha, source_ref,
    checked_out_sha, required_ci_result,
):
    """Automatic events can only select dry-run; rejected ones are no-ops."""
    automatic = event_name == "workflow_run"

    def reject(reason):
        if automatic:
            return {"schema": SCHEMA, "eligible": False, "mode": "skip", "reason": reason}
        raise ContextError(reason)

    if type(repository) is not str or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        return reject("invalid repository identity")
    if not re.fullmatch(r"[1-9][0-9]*", str(repository_id)):
        return reject("invalid repository ID")
    repository_id = int(repository_id)
    if type(source_sha) is not str or not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        return reject("invalid source SHA")
    if type(source_ref) is not str:
        return reject("invalid source ref")
    if checked_out_sha != source_sha:
        return reject("checked-out SHA differs from the workflow source")
    if not isinstance(payload, dict):
        return reject("invalid event payload")

    context = {
        "schema": SCHEMA,
        "eligible": True,
        "event": event_name,
        "source": {
            "repository": repository,
            "repository_id": repository_id,
            "sha": source_sha,
            "ref": source_ref,
            "checked_out_sha": checked_out_sha,
        },
        "required_ci_job_result": required_ci_result,
    }

    if automatic:
        # GITHUB_SHA for workflow_run is the default-branch context. Requiring
        # equality binds the checkout and built-in provenance to the CI SHA;
        # an old successful run cannot rehearse a newer, untested commit.
        if required_ci_result != "skipped":
            return reject("automatic rehearsal must reuse upstream CI")
        if source_ref != "refs/heads/main":
            return reject("automatic rehearsal requires the main ref")
        event_repository = payload.get("repository") or {}
        run = payload.get("workflow_run") or {}
        if not isinstance(event_repository, dict) or not isinstance(run, dict):
            return reject("invalid upstream workflow metadata")
        head_repository = run.get("head_repository") or {}
        if not isinstance(head_repository, dict):
            return reject("invalid upstream repository metadata")
        for candidate in (event_repository, head_repository):
            if (
                type(candidate.get("id")) is not int
                or candidate["id"] != repository_id
                or candidate.get("full_name") != repository
            ):
                return reject("upstream workflow is not from this repository")
        if run.get("name") != "ci" or run.get("path") != CI_PATH:
            return reject("upstream workflow is not the required CI workflow")
        if run.get("event") != "push" or run.get("head_branch") != "main":
            return reject("upstream workflow is not a main push")
        if run.get("status") != "completed" or run.get("conclusion") != "success":
            return reject("upstream CI did not complete successfully")
        if run.get("head_sha") != source_sha:
            return reject("upstream CI SHA is stale or differs from the workflow source")
        run_id = run.get("id")
        if type(run_id) is not int or run_id <= 0:
            return reject("invalid upstream run ID")
        run_attempt = run.get("run_attempt")
        if type(run_attempt) is not int or run_attempt <= 0:
            return reject("invalid upstream run attempt")
        run_url = f"https://github.com/{repository}/actions/runs/{run_id}"
        if run.get("html_url") != run_url:
            return reject("upstream run URL differs from its identity")
        context.update({
            "mode": "dry-run",
            "release_tag": "",
            "required_ci": {
                "authority": "upstream-ci",
                "result": "success",
                "run_id": run_id,
                "run_attempt": run_attempt,
                "url": run_url,
                "path": CI_PATH,
                "event": "push",
                "branch": "main",
                "head_sha": source_sha,
                "repository_id": repository_id,
                "repository": repository,
            },
        })
        return context

    # Tag pushes and manual requests retain the complete reusable CI gate.
    if required_ci_result != "success":
        return reject("tag and manual requests require successful reusable CI")
    if not source_ref.startswith(("refs/heads/", "refs/tags/")):
        return reject("invalid release ref")
    is_release_tag = source_ref.startswith("refs/tags/v")
    if event_name == "workflow_dispatch":
        inputs = payload.get("inputs") or {}
        if not isinstance(inputs, dict):
            return reject("invalid dispatch inputs")
        dry_run = inputs.get("dry_run")
        if type(dry_run) is bool:
            pass
        elif type(dry_run) is str and dry_run in ("true", "false"):
            dry_run = dry_run == "true"
        else:
            return reject("dispatch requires a boolean dry_run input")
        if dry_run:
            mode = "dry-run"
        elif inputs.get("confirm") == "publish" and is_release_tag:
            mode = "publish"
        else:
            return reject("dispatch upload requires a v* tag, dry_run=false and confirm=publish")
    elif event_name == "push" and is_release_tag:
        mode = "publish"
    else:
        return reject("unsupported release event or ref")
    context.update({
        "mode": mode,
        "release_tag": source_ref.removeprefix("refs/tags/") if is_release_tag else "",
        "required_ci": {"authority": "reusable-ci", "result": "success"},
    })
    return context


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify-context", action="store_true")
    parser.add_argument("--metadata-path", type=Path)
    args = parser.parse_args()
    try:
        payload = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        checkout_sha = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], text=True,
        ).strip()
        context = select_context(
            event_name=os.environ["GITHUB_EVENT_NAME"], payload=payload,
            repository=os.environ["GITHUB_REPOSITORY"],
            repository_id=os.environ["GITHUB_REPOSITORY_ID"],
            source_sha=os.environ["GITHUB_SHA"], source_ref=os.environ["GITHUB_REF"],
            checked_out_sha=checkout_sha,
            required_ci_result=os.environ.get("REQUIRED_CI_RESULT", ""),
        )
        context["workflow"] = {
            key: os.environ.get(environment, "")
            for key, environment in (
                ("ref", "GITHUB_WORKFLOW_REF"), ("sha", "GITHUB_WORKFLOW_SHA"),
                ("run_id", "GITHUB_RUN_ID"), ("run_attempt", "GITHUB_RUN_ATTEMPT"),
            )
        }
        if args.verify_context and context != json.loads(os.environ["RELEASE_CONTEXT_JSON"]):
            raise ContextError("release context changed between selection and packaging")
        if context["eligible"] and args.metadata_path:
            args.metadata_path.parent.mkdir(parents=True, exist_ok=True)
            args.metadata_path.write_text(json.dumps(context, indent=2, sort_keys=True) + "\n")
        output = os.environ.get("GITHUB_OUTPUT")
        if output:
            with Path(output).open("a") as destination:
                destination.write(f"eligible={str(context['eligible']).lower()}\n")
                destination.write(f"mode={context['mode']}\n")
                destination.write(f"source_sha={context.get('source', {}).get('sha', '')}\n")
                destination.write("context_json=" + json.dumps(context, sort_keys=True) + "\n")
        print(f"Release context: {context['mode']}")
        return 0
    except (ContextError, KeyError, OSError, ValueError, subprocess.CalledProcessError) as error:
        # Report the policy error without dumping event payloads or credentials.
        print(f"::error::release context rejected: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
