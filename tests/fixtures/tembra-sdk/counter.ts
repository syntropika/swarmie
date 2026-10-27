import { orb, type Context, type State } from "@syntropika/tembra-sdk";

/** A standalone authoring fixture, independent of PluginManager and Durable Tasks. */
export default orb({
  name: "fixture.sdk-counter",
  apiVersion: 1,
  services: {
    "fixture.sdk-counter": {
      create(self) {
        return self.state(0);
      },
      methods: {
        read(state: State<number>) {
          return state.value;
        },
        increment(state: State<number>, context: Context, amount: number) {
          return state.update(context, (value) => value + amount);
        },
      },
    },
  },
});
