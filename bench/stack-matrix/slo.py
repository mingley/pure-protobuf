"""SB-11 sustained-QPS search: step offered load until the p99 SLO breaks.

A step is valid only when every offered call is accounted for (no hidden
drops), nothing failed or timed out, nothing was rejected at the
generator cap, the generator itself was not saturated, and e2e p99
(schedule-relative, so coordinated omission cannot hide a stall) is at
or under the SLO. The sustained rate is the highest valid step; the
first invalid step is retained as the ceiling witness.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List


@dataclass
class StepResult:
    offered_rate: float
    offered_calls: int = 0
    successful_calls: int = 0
    failed_calls: int = 0
    timed_out_calls: int = 0
    rejected_calls: int = 0
    success_qps: float = 0.0
    p99_s: float = float("inf")
    p50_s: float = float("inf")
    gen_saturated: bool = False
    valid: bool = False
    invalid_reason: str = ""

    def as_dict(self) -> Dict[str, Any]:
        return {
            "offered_rate": self.offered_rate,
            "offered_calls": self.offered_calls,
            "successful_calls": self.successful_calls,
            "failed_calls": self.failed_calls,
            "timed_out_calls": self.timed_out_calls,
            "rejected_calls": self.rejected_calls,
            "success_qps": self.success_qps,
            "p99_s": self.p99_s,
            "p50_s": self.p50_s,
            "gen_saturated": self.gen_saturated,
            "valid": self.valid,
            "invalid_reason": self.invalid_reason,
        }


@dataclass
class SloResult:
    sustained_qps: float = 0.0
    sustained_step: int = -1
    steps: List[StepResult] = field(default_factory=list)
    ceiling_reason: str = ""

    def as_dict(self) -> Dict[str, Any]:
        return {
            "sustained_qps": self.sustained_qps,
            "sustained_step": self.sustained_step,
            "steps": [s.as_dict() for s in self.steps],
            "ceiling_reason": self.ceiling_reason,
        }


def check_step(step: StepResult, slo_p99_s: float) -> StepResult:
    """Rate one probe step against the SLO; pure, unit-testable."""
    accounted = (
        step.successful_calls + step.failed_calls + step.timed_out_calls
    )
    if step.offered_calls <= 0:
        step.invalid_reason = "no offered calls recorded"
    elif accounted + step.rejected_calls != step.offered_calls:
        step.invalid_reason = (
            f"unaccounted calls: offered={step.offered_calls} "
            f"completed={accounted} rejected={step.rejected_calls}"
        )
    elif step.failed_calls or step.timed_out_calls:
        step.invalid_reason = (
            f"errors: failed={step.failed_calls} timed_out={step.timed_out_calls}"
        )
    elif step.rejected_calls:
        step.invalid_reason = (
            f"generator cap rejected {step.rejected_calls} offered calls"
        )
    elif step.gen_saturated:
        step.invalid_reason = "load generator saturated"
    elif step.p99_s > slo_p99_s:
        step.invalid_reason = (
            f"p99 {step.p99_s * 1000:.2f}ms over SLO {slo_p99_s * 1000:.2f}ms"
        )
    else:
        step.valid = True
    return step


def find_sustained_qps(
    probe: Callable[[float], StepResult],
    *,
    start_rate: float,
    max_rate: float,
    growth: float = 2.0,
    slo_p99_s: float = 0.010,
    max_steps: int = 12,
) -> SloResult:
    """Step offered load geometrically; return the highest SLO-valid step.

    Stops at the first invalid step (kept as the ceiling witness) or at
    max_rate/max_steps. A zero sustained rate with a ceiling reason is a
    measured outcome, not a harness failure: callers record it.
    """
    if start_rate <= 0 or max_rate < start_rate or growth <= 1.0:
        raise ValueError("need 0 < start_rate <= max_rate and growth > 1")
    result = SloResult()
    rate = start_rate
    for _ in range(max_steps):
        step = check_step(probe(rate), slo_p99_s)
        result.steps.append(step)
        if step.valid:
            result.sustained_qps = step.success_qps
            result.sustained_step = len(result.steps) - 1
        else:
            result.ceiling_reason = (
                f"rate {rate:g}: {step.invalid_reason}" if step.invalid_reason else f"rate {rate:g}"
            )
            return result
        if rate >= max_rate:
            result.ceiling_reason = f"reached max_rate {max_rate:g} without a ceiling"
            return result
        rate = min(rate * growth, max_rate)
    result.ceiling_reason = f"reached max_steps {max_steps} without a ceiling"
    return result
