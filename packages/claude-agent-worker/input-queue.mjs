export class InputQueue {
  values = [];
  waiter = null;
  ended = false;
  push(value) {
    if (this.ended) {
      throw new Error("Claude SDK input is closed");
    }
    if (this.waiter) {
      const { waiter } = this;
      this.waiter = null;
      waiter({ value, done: false });
    } else {
      this.values.push(value);
    }
  }
  close() {
    this.ended = true;
    this.waiter?.({ done: true });
    this.waiter = null;
  }
  [Symbol.asyncIterator]() {
    return this;
  }
  next() {
    if (this.values.length) {
      return Promise.resolve({ value: this.values.shift(), done: false });
    }
    if (this.ended) {
      return Promise.resolve({ done: true });
    }
    const pending = Promise.withResolvers();
    this.waiter = pending.resolve;
    return pending.promise;
  }
}
