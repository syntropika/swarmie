import { consumer } from "./dynamic-consumer";

/** Negative fixture: native Counter must refuse this argument before provider mutation. */
export default consumer(-1);
