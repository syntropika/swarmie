"""Aggregate elapsed-time stack samples without pretending they are CPU percentages."""

from collections import Counter


def aggregate(samples):
    inclusive = Counter()
    top = Counter()
    unresolved = truncated = errors = 0
    for sample in samples:
        frames = sample["frames"]
        names = [frame.get("name") or "<unresolved>" for frame in frames]
        if names:
            top[names[0]] += 1
            inclusive.update(set(names))
        unresolved += any(name == "<unresolved>" for name in names) or not names
        truncated += bool(sample.get("truncated"))
        errors += bool(sample.get("unwind_error"))
    def rows(counter):
        return [{"function": name, "samples": count,
                 "sample_fraction": count / len(samples)} for name, count in counter.most_common()]
    return {"sample_count": len(samples), "unresolved_samples": unresolved,
            "truncated_samples": truncated, "unwind_error_samples": errors,
            "inclusive": rows(inclusive), "top_frame": rows(top),
            "interpretation": "Elapsed-time interrupted runtime-thread stack presence, including waits; not CPU cost or exclusive time. Inclusive functions overlap."}
