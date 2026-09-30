import { describe, expect, it } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import { FinishedList } from "./FinishedList";
import { NOW, session } from "../test/fixtures";
import { useMonitor } from "../store/useMonitor";
import { HARNESS_CODE } from "../types";

/** An ended row: dead process, state pinned to ended, nothing jumpable. */
function ended(overrides = {}) {
  return session({
    state: "ended",
    liveness: "dead",
    state_changed_at: NOW - 3 * 86_400_000,
    started_at: NOW - 3 * 86_400_000 - 45 * 60_000,
    ...overrides,
  });
}

function list(rows = [ended()], store: Record<string, unknown> = {}) {
  useMonitor.setState({ query: "", dateFilter: "all", now: NOW, ...store });
  return render(<FinishedList ended={rows} now={NOW} />);
}

/**
 * Row names, read off the rendered rows. Scoped to the container rather than
 * to `getAllByTitle`, because the rows carry nested spans that repeat the same
 * tooltip - a document-wide title search finds six nodes per row.
 */
function names(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>("span.text-xs")).map(
    (el) => el.textContent ?? "",
  );
}

/** The row divs, i.e. everything the list rendered as a session. */
function rows(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>("div.rounded-xl"));
}

describe("FinishedList", () => {
  it("says so when the machine has no history to show", () => {
    list([]);
    expect(screen.getByText(/nothing finished yet/)).toBeInTheDocument();
  });

  it("orders newest first, across harnesses", () => {
    // The live list groups by harness; a date range and a search box are both
    // linear, so history is one flat reverse-chronological list.
    const { container } = list([
      ended({ name: "oldest", state_changed_at: NOW - 10 * 86_400_000 }),
      ended({ name: "newest", state_changed_at: NOW - 60_000 }),
      ended({ name: "middle", state_changed_at: NOW - 2 * 86_400_000 }),
    ]);
    expect(names(container)).toEqual(["newest", "middle", "oldest"]);
  });

  it("never shows a state chip, because the file still lies about a dead process", () => {
    // A row frozen at `busy` since March is not calmly waiting for anyone.
    list([ended({ state_changed_at: NOW - 60 * 86_400_000 })]);
    expect(screen.getByText("ended")).toBeInTheDocument();
    expect(screen.queryByText("idle")).not.toBeInTheDocument();
    expect(screen.queryByText("running")).not.toBeInTheDocument();
  });

  it("explains a row whose process could never be verified", () => {
    // The macOS case: no procfs, so the session is filed rather than shown as
    // live, and the row has to say why it is not in the live list.
    const { container } = list([ended({ liveness: "unknown" })]);
    expect(container.innerHTML).toContain("no verifiable process id");
  });

  it("explains a presence-only row, which has nothing behind it but an id", () => {
    const { container } = list([ended({ tier: "presence-only" })]);
    expect(container.innerHTML).toContain("nothing is known about this conversation");
  });

  it("gives the ordinary case the ordinary reason", () => {
    const { container } = list([ended()]);
    expect(container.innerHTML).toContain("the process is gone");
  });

  it("prints how long ago it was touched, and not how long it ran, by default", () => {
    list([ended()]);
    expect(screen.getByText("3d ago")).toBeInTheDocument();
  });

  it("prints a span only when there is a span worth printing", () => {
    // "ran 0s" on every row is noise that crowds out the path.
    const { container } = list([ended()]);
    expect(container.innerHTML).toContain("ran 45m");
  });

  it("omits the span when the harness stamped both instants identically", () => {
    // Several harnesses set started_at to the same moment as the last write.
    const { container } = list([ended({ started_at: NOW - 3 * 86_400_000 })]);
    expect(container.innerHTML).not.toContain("ran ");
  });

  it("omits the span when it is too short to mean anything", () => {
    const { container } = list([ended({ started_at: NOW - 3 * 86_400_000 - 59_000 })]);
    expect(container.innerHTML).not.toContain("ran ");
  });

  it("omits the span when the harness never recorded a start", () => {
    // started_at of 0 is "unknown", not the epoch.
    const { container } = list([ended({ started_at: 0 })]);
    expect(container.innerHTML).not.toContain("ran ");
  });

  it("keeps the model and the harness code on the row", () => {
    // Which harness it was, on the row, is one glance - the grouping headers
    // were the thing this list gave up to stay scannable.
    const { container } = list([ended({ model: "gpt-5-codex" })]);
    expect(screen.getByText(HARNESS_CODE["claude-code"])).toBeInTheDocument();
    expect(screen.getByText("gpt-5-codex")).toBeInTheDocument();
    expect(container).toBeTruthy();
  });

  it("shows tokens and cost only when there are any", () => {
    const { container } = list([
      ended({
        tokens: { input: 0, output: 0, reasoning: 0, cache_read: 0, cache_write: 0 },
        cost: 0,
      }),
    ]);
    expect(container.innerHTML).not.toContain("tok");
    expect(container.innerHTML).not.toContain("$");
  });

  it("shows tokens and cost when the harness reported them", () => {
    const { container } = list([
      ended({
        tokens: { input: 1_000, output: 2_000, reasoning: 0, cache_read: 0, cache_write: 0 },
        cost: 1.5,
      }),
    ]);
    expect(screen.getByText(/3k tok/)).toBeInTheDocument();
    expect(screen.getByText(/\$1\.50/)).toBeInTheDocument();
    expect(container).toBeTruthy();
  });

  it("offers no jump, because a finished session has no live process", () => {
    // The pane that hosted it is usually gone too, so a *jump* button here
    // could not work - it would be a lie in the shape of a control. The row
    // does have one button, the one that reopens the conversation, and it is
    // labelled so the two can never be confused.
    const { container } = list([ended({ jump_target: "wA:p1" })]);
    expect(container.innerHTML).not.toContain("Jump to this session");
    expect(
      within(rows(container)[0] as HTMLElement).getByRole("button", {
        name: /Reopen this conversation/,
      }),
    ).toBeInTheDocument();
  });

  it("counts what the date range hid", () => {
    list(
      [
        ended({ name: "old", state_changed_at: NOW - 40 * 86_400_000 }),
        ended({ name: "new", state_changed_at: NOW - 60_000 }),
      ],
      { dateFilter: "month" },
    );
    expect(screen.getByText("1 of 2 sessions")).toBeInTheDocument();
  });

  it("says when a range is empty, and names the range that emptied it", () => {
    list([ended({ state_changed_at: NOW - 40 * 86_400_000 })], { dateFilter: "today" });
    expect(screen.getByText("nothing in this date range")).toBeInTheDocument();
  });

  it("distinguishes an empty range from an empty search", () => {
    list([ended()], { query: "nothing-matches-this" });
    expect(screen.getByText(/nothing matching “nothing-matches-this”/)).toBeInTheDocument();
  });

  it("filters by the query as well as the range", () => {
    const { container } = list([ended({ name: "migrations" }), ended({ name: "unrelated" })], {
      query: "migrat",
    });
    expect(names(container)).toEqual(["migrations"]);
  });

  it("renders rows as a scrollable list, since a long history has to fit", () => {
    const { container } = list([ended()]);
    const scroller = container.firstElementChild;
    expect(scroller?.className).toContain("overflow-y-auto");
  });

  it("keys rows by the process, not the session id, which repeats on resume", () => {
    // Two processes that reused one session id must not collapse into one row.
    const { container } = list([
      ended({ session_id: "same", key: { pid: 111 } }),
      ended({ session_id: "same", key: { pid: 222 } }),
    ]);
    expect(rows(container)).toHaveLength(2);
    expect(within(rows(container)[0] as HTMLElement).getByText(HARNESS_CODE["claude-code"])).toBeInTheDocument();
  });
});

describe("FinishedList: reopening a finished session", () => {
  it("offers the reopen button on a resumable harness", () => {
    const { container } = list([ended({ key: { harness: "claude-code" } })]);
    const button = within(rows(container)[0] as HTMLElement).getByRole("button");
    expect(button).toBeEnabled();
    // The tooltip names the command, so nobody has to guess which of three
    // nearly-identical flags is about to be run.
    expect(button.getAttribute("title")).toContain("claude --resume");
  });

  it("disables it for gemini and says exactly why", () => {
    // "Latest" would be the easy thing to offer here and it would open the
    // wrong conversation. The reason is on the button because a greyed control
    // with no explanation is a bug report.
    const { container } = list([ended({ key: { harness: "gemini" } })]);
    const button = within(rows(container)[0] as HTMLElement).getByRole("button");
    expect(button).toBeDisabled();
    expect(button.getAttribute("title")).toMatch(/latest/);
  });

  it("disables it for antigravity, whose cwd is not even a project", () => {
    const { container } = list([
      ended({ key: { harness: "antigravity" }, tier: "presence-only" }),
    ]);
    expect(within(rows(container)[0] as HTMLElement).getByRole("button")).toBeDisabled();
  });

  it("disables it when the row has no directory to reopen it in", () => {
    const { container } = list([ended({ cwd: "" })]);
    const button = within(rows(container)[0] as HTMLElement).getByRole("button");
    expect(button).toBeDisabled();
    expect(button.getAttribute("title")).toMatch(/working directory/);
  });

  it("passes the row's own identity to the store, not the first row's", () => {
    // Two rows, two different sessions: clicking the second must ask for the
    // second. Getting this wrong reopens a conversation nobody asked for.
    const calls: unknown[][] = [];
    useMonitor.setState({
      query: "",
      dateFilter: "all",
      now: NOW,
      rerunSession: async (...args: unknown[]) => {
        calls.push(args);
      },
    });
    const a = ended({ name: "alpha", session_id: "aaa", key: { harness: "claude-code" } });
    const b = ended({ name: "beta", session_id: "bbb", key: { harness: "codex" } });
    const { container } = render(<FinishedList ended={[a, b]} now={NOW} />);
    within(rows(container)[1] as HTMLElement).getByRole("button").click();
    expect(calls).toEqual([["codex", "bbb", a.cwd]]);
  });

  it("marks the row busy while the launch is in flight, and unmarks it after", async () => {
    // Two launches are legitimate, so this is per row rather than one global
    // flag - and the row must not be left spinning if the launch fails.
    let release: () => void = () => {};
    useMonitor.setState({
      query: "",
      dateFilter: "all",
      now: NOW,
      rerunSession: () => new Promise<void>((resolve) => (release = resolve)),
    });
    const a = ended({ name: "alpha", key: { pid: 1 } });
    const b = ended({ name: "beta", key: { pid: 2 } });
    const { container } = render(<FinishedList ended={[a, b]} now={NOW} />);
    const all = () => Array.from(container.querySelectorAll("button"));

    all()[0].click();
    // waitFor rather than a fixed number of microtasks: how many renders a
    // promise resolution needs is React's business, not this test's.
    await waitFor(() => expect(all()[0].getAttribute("aria-busy")).toBe("true"));
    expect(all()[0]).toBeDisabled();
    // The other row is untouched: waiting on one session is not a reason to
    // lock up the whole list.
    expect(all()[1].getAttribute("aria-busy")).toBe("false");
    expect(all()[1]).toBeEnabled();

    release();
    await waitFor(() => expect(all()[0].getAttribute("aria-busy")).toBe("false"));
    expect(all()[0]).toBeEnabled();
  });

  it("will not start the same session twice from one click", () => {
    // Two clicks in the same tick, before React has re-rendered to disable the
    // button. Waiting on the re-render would be the obvious way to write this
    // and it is not a guard at all - two terminals for one conversation is not
    // a cosmetic bug.
    let calls = 0;
    const { container } = list([ended()], {
      rerunSession: () => {
        calls += 1;
        return new Promise<void>(() => {});
      },
    });
    const button = within(rows(container)[0] as HTMLElement).getByRole("button");
    button.click();
    button.click();
    button.click();
    expect(calls).toBe(1);
  });
});
