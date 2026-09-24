/** A modal-owned lane keeps at most one command active and one latest request
 * pending. Cancelling pending work does not pretend to stop an active core read. */
export class CitationReadLane<T> {
  private active = false;
  private closed = false;
  private pending: {
    run: () => Promise<T>;
    resolve: (value: T) => void;
    reject: (cause: Error) => void;
  } | null = null;

  open() {
    this.closed = false;
  }
  clearPending() {
    this.pending?.reject(
      new Error("Citation read superseded before dispatch."),
    );
    this.pending = null;
  }
  close() {
    this.closed = true;
    this.clearPending();
  }
  read(run: () => Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      if (this.closed) {
        reject(new Error("Citation reader is closed."));
        return;
      }
      this.clearPending();
      this.pending = { run, resolve, reject };
      this.dispatch();
    });
  }
  private dispatch() {
    if (this.active || this.closed || !this.pending) return;
    const request = this.pending;
    this.pending = null;
    this.active = true;
    void Promise.resolve()
      .then(request.run)
      .then(request.resolve, request.reject)
      .finally(() => {
        this.active = false;
        this.dispatch();
      });
  }
}
