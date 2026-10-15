import type { ComponentPropsWithRef } from "react";

import { Icon } from "../../atoms/icon";
import { cn } from "../../classes";

export type ComparisonFeature = { id: string; label: string; values: readonly boolean[] };

/** Equal-width feature matrix; values correspond to the caller's ordered plans. */
export function ComparisonTable({
  plans,
  features,
  featureLabel,
  includedLabel,
  excludedLabel,
  className,
  recommendedPlan,
  recommendationLabel,
  reason,
  ...props
}: ComponentPropsWithRef<"table"> & {
  plans: readonly string[];
  recommendedPlan?: string;
  recommendationLabel?: string;
  reason?: string;
  features: readonly ComparisonFeature[];
  featureLabel: string;
  includedLabel: string;
  excludedLabel: string;
}) {
  return (
    <div className="min-w-0 overflow-hidden rounded-panel bg-surface-muted">
      {reason === undefined ? null : <p className="p-3 text-ui">{reason}</p>}
      <table {...props} className={cn("w-full table-fixed text-ui leading-ui", className)}>
        <thead className="text-text-muted">
          <tr>
            <th scope="col" className="px-3 py-2 text-left font-medium">
              {featureLabel}
            </th>
            {plans.map((plan) => (
              <th
                key={plan}
                scope="col"
                className={cn(
                  "px-3 py-2 text-left font-medium transition-colors motion-reduce:transition-none",
                  plan === recommendedPlan && "bg-primary-soft text-primary",
                )}
              >
                {plan}
                {plan === recommendedPlan ? (
                  <span className="mt-1 block text-xs">{recommendationLabel}</span>
                ) : null}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="bg-surface">
          {features.map((feature) => (
            <tr key={feature.id} className="even:bg-surface-muted/40">
              <th scope="row" className="px-3 py-2 text-left font-normal break-words">
                {feature.label}
              </th>
              {plans.map((plan, index) => (
                <td key={plan} className="px-3 py-2">
                  {feature.values[index] === true ? (
                    <Icon name="check" className="size-4 text-success" />
                  ) : (
                    <span aria-hidden="true" className="text-text-muted">
                      —
                    </span>
                  )}
                  <span className="sr-only">
                    {feature.values[index] === true ? includedLabel : excludedLabel}
                  </span>
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
