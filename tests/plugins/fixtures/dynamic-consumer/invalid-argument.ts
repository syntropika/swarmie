import { consumer } from "../../../../examples/plugins/dynamic-consumer/facet";

/** Negative fixture: native Counter must refuse this argument before provider mutation. */
export default consumer(-1);
