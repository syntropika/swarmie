/** Shared signed chart coordinates; positive and negative stacks accumulate independently. */
export function chartGeometry(
  labels: readonly string[],
  series: readonly { values: readonly number[] }[],
  stacked: boolean,
  bars: boolean,
) {
  const baseline = (seriesIndex: number, index: number) => {
    if (!stacked) {
      return 0;
    }
    const negative = (series[seriesIndex]?.values[index] ?? 0) < 0;
    let sum = 0;
    for (const entry of series.slice(0, seriesIndex)) {
      const value = entry.values[index] ?? 0;
      if (value < 0 === negative) {
        sum += value;
      }
    }
    return sum;
  };
  const ends = series.flatMap((entry, seriesIndex) =>
    entry.values.map((value, index) => value + baseline(seriesIndex, index)),
  );
  const minimum = Math.min(0, ...ends);
  const maximum = Math.max(0, ...ends);
  const span = maximum - minimum || 1;
  const y = (value: number) => 200 - ((value - minimum) / span) * 160;
  const categoryWidth = 520 / Math.max(1, labels.length);
  const x = (index: number) =>
    bars ? 40 + (index + 0.5) * categoryWidth : 40 + index * (520 / Math.max(1, labels.length - 1));
  return { baseline, minimum, maximum, y, x, categoryWidth };
}
