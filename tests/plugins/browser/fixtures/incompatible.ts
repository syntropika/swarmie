import { defineFacet, local, method, Type } from "@tembra/sdk";

const Incompatible = local("swarmie.acceptance.widget", {
  version: 1,
  input: Type.Object({}),
  members: { increment: method(undefined, Type.String()) },
});

export default defineFacet({
  name: "browser-widget",
  uses: [],
  setup(_scope, declaration) {
    declaration.serve(Incompatible, () => ({
      state: {},
      methods: { increment: () => "incompatible" },
    }));
  },
});
