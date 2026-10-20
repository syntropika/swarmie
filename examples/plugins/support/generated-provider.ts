import type {
  Completion,
  ContractReference,
  FacetDeclaration,
  GeneratedContract,
  InputOf,
  ManagedContext,
  ObjectDeclaration,
  Readable,
  ReferenceOf,
  ServeOptions,
} from "@tembra/sdk";

type ProviderMethod<T> = T extends (...args: infer Args) => Promise<infer Result>
  ? Args extends [infer Payload, ...unknown[]]
    ? (context: ManagedContext, payload: Payload) => Completion<Result>
    : (context: ManagedContext) => Completion<Result>
  : never;

type Provider<R> = {
  readonly variant?: string;
  readonly state: {
    readonly [
      K in Exclude<keyof R, keyof ContractReference> as R[K] extends Readable<unknown> ? K : never
    ]: R[K];
  };
  readonly methods: {
    readonly [
      K in Exclude<keyof R, keyof ContractReference> as R[K] extends (
        ...args: never[]
      ) => Promise<unknown>
        ? K
        : never
    ]: ProviderMethod<R[K]>;
  };
};

/** The pinned generator emits consumer metadata; core still validates the provider descriptor. */
export function serveGenerated<C extends GeneratedContract<unknown, ContractReference>>(
  declaration: FacetDeclaration,
  contract: C,
  factory: (input: InputOf<C>, self: ObjectDeclaration) => Provider<ReferenceOf<C>>,
  options?: ServeOptions,
): void {
  // Generated References define the precise provider members above. The pinned SDK's
  // consumer-only tokens omit provider inference. Dispatch only its actual serve method;
  // Tembra validates the canonical descriptor, factory members, State and operation results.
  Reflect.apply(declaration.serve.bind(declaration), declaration, [contract, factory, options]);
}
