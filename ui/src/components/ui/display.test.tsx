// UI kit, display components: variants, semantic classes (tokens, never raw colours), text that
// never relies on colour alone, and axe-clean markup.
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { axeViolations } from "../../test/axe";
import {
  Avatar,
  Button,
  buttonClasses,
  Card,
  EmptyState,
  EnsoMark,
  Eyebrow,
  GateLight,
  IconButton,
  IconTile,
  initials,
  Kbd,
  KindTag,
  MonoId,
  ReadinessRing,
  readinessTone,
  Skeleton,
  SkeletonList,
  StatusPill,
  PILL_STATUSES,
  KINDS,
} from ".";

afterEach(cleanup);

const classOf = (element: Element) => element.getAttribute("class") ?? "";

describe("Button", () => {
  it("renders every variant with its semantic classes and the pill sizes 30/34/38/44", () => {
    expect(buttonClasses({ variant: "primary", size: 38 })).toContain("bg-primary");
    expect(buttonClasses({ variant: "accent", size: 34 })).toContain("bg-accent");
    expect(buttonClasses({ variant: "accent", size: 34 })).toContain("hover:bg-accent-hover");
    expect(buttonClasses({ variant: "accent", size: 34 })).toContain("active:bg-accent-pressed");
    expect(buttonClasses({ variant: "outline", size: 30 })).toContain("border-line");
    expect(buttonClasses({ variant: "destructive", size: 38 })).toContain("bg-blocked");
    expect(buttonClasses({ variant: "ghost", size: 34 })).toContain("bg-transparent");
    expect(buttonClasses({ variant: "inverse", size: 44 })).toContain("bg-on-hero");
    expect(buttonClasses({ size: 30 })).toContain("h-[30px]");
    expect(buttonClasses({ size: 34 })).toContain("h-[34px]");
    expect(buttonClasses({ size: 38 })).toContain("h-[38px]");
    expect(buttonClasses({ size: 44 })).toContain("h-11");
    expect(buttonClasses({})).toContain("rounded-pill");
    expect(buttonClasses({ className: "extra" })).toContain("extra");
  });

  it("is a typed button with optional icons and a keyboard hint, and does nothing while pending", async () => {
    const onClick = vi.fn();
    const { rerender } = render(
      <Button variant="accent" icon={<span data-testid="lead" />} trailingIcon={<span data-testid="trail" />} shortcut="⌘↩" onClick={onClick}>
        Review
      </Button>,
    );
    const button = screen.getByRole("button", { name: /Review/ });
    expect(button.getAttribute("type")).toBe("button");
    expect(screen.getByTestId("lead")).toBeTruthy();
    expect(screen.getByTestId("trail")).toBeTruthy();
    expect(button.textContent).toContain("⌘↩");
    await userEvent.click(button);
    expect(onClick).toHaveBeenCalledTimes(1);
    rerender(
      <Button pending onClick={onClick}>
        Review
      </Button>,
    );
    const pending = screen.getByRole("button", { name: /Review/ });
    expect(pending.getAttribute("aria-busy")).toBe("true");
    expect((pending as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(pending);
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(await axeViolations()).toEqual([]);
  });
});

describe("IconButton", () => {
  it("has an accessible name, a pressed state and the 34/30 square sizes", async () => {
    const onClick = vi.fn();
    const { rerender } = render(<IconButton label="Assistant" icon={<svg />} onClick={onClick} />);
    const button = screen.getByRole("button", { name: "Assistant" });
    expect(classOf(button)).toContain("size-[34px]");
    expect(button.getAttribute("aria-pressed")).toBeNull();
    await userEvent.click(button);
    expect(onClick).toHaveBeenCalled();
    rerender(<IconButton label="Assistant" icon={<svg />} pressed size={30} variant="outline" />);
    const pressed = screen.getByRole("button", { name: "Assistant" });
    expect(pressed.getAttribute("aria-pressed")).toBe("true");
    expect(classOf(pressed)).toContain("size-[30px]");
    expect(classOf(pressed)).toContain("border-line");
    expect(await axeViolations()).toEqual([]);
  });
});

describe("StatusPill", () => {
  it("renders every status with an icon and its catalogue text, toned by meaning", async () => {
    const { container } = render(
      <div>
        {PILL_STATUSES.map((status) => (
          <StatusPill key={status} status={status} />
        ))}
      </div>,
    );
    const pills = [...container.querySelectorAll("[data-status]")];
    expect(pills).toHaveLength(PILL_STATUSES.length);
    for (const pill of pills) {
      expect(pill.querySelector("svg")).not.toBeNull();
      expect(pill.textContent?.trim().length).toBeGreaterThan(0);
    }
    const pill = (status: string) => container.querySelector(`[data-status='${status}']`) as Element;
    expect(pill("accepted").textContent).toBe("Accepted");
    expect(classOf(pill("pass"))).toContain("bg-pass-tint");
    expect(classOf(pill("pass"))).toContain("text-pass-fg");
    expect(classOf(pill("fail"))).toContain("text-blocked-fg");
    expect(classOf(pill("stale"))).toContain("bg-attention-tint");
    expect(classOf(pill("running"))).toContain("bg-running-tint");
    expect(classOf(pill("undecidable"))).toContain("bg-neutral-tint");
    expect(await axeViolations()).toEqual([]);
  });

  it("takes an explicit label and tone", () => {
    render(<StatusPill status="confirmed" tone="neutral" label="Confirmed by HR" />);
    const pill = screen.getByText("Confirmed by HR").closest("[data-status]") as Element;
    expect(classOf(pill)).toContain("bg-neutral-tint");
  });
});

describe("GateLight", () => {
  it("shows each gate state with a dot, the plain name, the code and the state text", async () => {
    const { container } = render(
      <ul>
        <li>
          <GateLight gate="I0" state="pass" />
        </li>
        <li>
          <GateLight gate="F1" state="fail" detail="2 blocking issues" />
        </li>
        <li>
          <GateLight gate="F2" state="stale" />
        </li>
        <li>
          <GateLight gate="F3" state="not_evaluated" showCode={false} />
        </li>
        <li>
          <GateLight gate="F4" state="running" />
        </li>
      </ul>,
    );
    const light = (gate: string) => container.querySelector(`[data-gate='${gate}']`) as HTMLElement;
    expect(light("I0").textContent).toContain("Evidence");
    expect(light("I0").textContent).toContain("I0");
    expect(light("I0").textContent).toContain("Passes");
    expect(light("F1").textContent).toContain("Blocked · 2 blocking issues");
    expect(light("F2").textContent).toContain("Outdated");
    expect(light("F3").textContent).toContain("Not evaluated");
    expect(light("F3").textContent).not.toContain("F3");
    expect(light("F4").textContent).toContain("Checking…");
    const dot = (gate: string) => light(gate).querySelector("[data-gate-dot]") as Element;
    expect(classOf(dot("I0"))).toContain("bg-pass");
    expect(classOf(dot("F1"))).toContain("bg-blocked");
    // Outdated is a hollow ring, never a filled colour.
    expect(classOf(dot("F2"))).toContain("border-stale");
    expect(classOf(dot("F2"))).not.toContain("bg-");
    expect(await axeViolations()).toEqual([]);
  });

  it("uses a custom label", () => {
    render(<GateLight gate="F1" state="pass" label="Requirement check" size={12} />);
    expect(screen.getByText("Requirement check")).toBeTruthy();
  });
});

describe("ReadinessRing", () => {
  it("colours by the handoff thresholds: >=85 midnight, >=70 slate, else amber", () => {
    expect(readinessTone(86)).toBe("primary");
    expect(readinessTone(85)).toBe("primary");
    expect(readinessTone(84)).toBe("muted");
    expect(readinessTone(70)).toBe("muted");
    expect(readinessTone(69)).toBe("attention");
  });

  it("draws the value arc with an accessible label, the centre text and sub-scores", async () => {
    const { container, rerender } = render(
      <ReadinessRing value={86} size={96} stroke={8} showLabel subScores={{ clarity: 92, completeness: 84, confirmation: null, consistency: 95, verifiability: 81 }} />,
    );
    const ring = screen.getByRole("img", { name: "Readiness 86%" });
    expect(ring.textContent).toContain("86%");
    expect(ring.textContent).toContain("ready");
    const arc = container.querySelector("[data-ring-value]") as SVGCircleElement;
    expect(arc.getAttribute("class")).toContain("stroke-primary");
    const r = (96 - 8) / 2;
    const c = 2 * Math.PI * r;
    expect(arc.getAttribute("stroke-dasharray")).toBe(`${(c * 86) / 100} ${c}`);
    expect(container.querySelector("[data-subscores]")?.textContent).toContain("Clarity");
    expect(await axeViolations()).toEqual([]);
    rerender(<ReadinessRing value={64} size={20} stroke={3} />);
    expect(container.querySelector("[data-ring-value]")?.getAttribute("class")).toContain("stroke-attention");
    rerender(<ReadinessRing value={null} size={20} stroke={3} />);
    expect(screen.getByRole("img", { name: "Readiness not calculated" })).toBeTruthy();
    expect(container.querySelector("[data-ring-value]")).toBeNull();
    rerender(<ReadinessRing value={140} size={72} stroke={6} showLabel />);
    expect(screen.getByRole("img", { name: "Readiness 100%" })).toBeTruthy();
  });
});

describe("text primitives", () => {
  it("renders Eyebrow, MonoId, Kbd, KindTag and IconTile with their styles", async () => {
    render(
      <div>
        <Eyebrow>Overview</Eyebrow>
        <Eyebrow tone="onDark" as="h2">
          Next action
        </Eyebrow>
        <MonoId>HR-009</MonoId>
        <MonoId size={11} tone="default" title="Requirement HR-010">
          HR-010
        </MonoId>
        <Kbd>⌘K</Kbd>
        {KINDS.map((kind) => (
          <KindTag key={kind} kind={kind} />
        ))}
        <KindTag kind="Security" withIcon={false} />
        <KindTag kind="custom-kind" />
        <IconTile icon={<svg />} />
        <IconTile icon={<svg />} selected size={56} />
      </div>,
    );
    expect(classOf(screen.getByText("Overview"))).toContain("tracking-eyebrow");
    expect(classOf(screen.getByText("Overview"))).toContain("uppercase");
    expect(screen.getByText("Next action").tagName).toBe("H2");
    expect(classOf(screen.getByText("Next action"))).toContain("text-hero-sub");
    expect(classOf(screen.getByText("HR-009"))).toContain("font-mono");
    expect(classOf(screen.getByText("HR-010"))).toContain("text-fg");
    expect(screen.getByText("⌘K").tagName).toBe("KBD");
    expect(screen.getByText("Functional")).toBeTruthy();
    expect(screen.getAllByText("Security")).toHaveLength(2);
    expect(screen.getByText("custom-kind")).toBeTruthy();
    expect(document.querySelectorAll("[data-icon-tile]")).toHaveLength(2);
    expect(await axeViolations()).toEqual([]);
  });

  it("renders Card variants", () => {
    const { container } = render(
      <div>
        <Card>Plain</Card>
        <Card as="section" padding="lg" radius="tile" elevation="xs" interactive selected aria-label="Gate tile">
          Tile
        </Card>
        <Card padding="none">None</Card>
        <Card padding="sm">Small</Card>
      </div>,
    );
    const [plain, tile, none, small] = [...container.querySelectorAll("[data-card]")];
    expect(classOf(plain as Element)).toContain("rounded-card");
    expect(classOf(plain as Element)).toContain("shadow-sm");
    expect(classOf(plain as Element)).toContain("p-6");
    expect((tile as Element).tagName).toBe("SECTION");
    expect(classOf(tile as Element)).toContain("rounded-tile");
    expect(classOf(tile as Element)).toContain("hover:shadow-md");
    expect(classOf(tile as Element)).toContain("border-primary");
    expect(classOf(none as Element)).not.toContain("p-6");
    expect(classOf(small as Element)).toContain("p-4");
  });

  it("renders Avatar initials and the enso mark", async () => {
    expect(initials("Marta Nowak")).toBe("MN");
    expect(initials("alice")).toBe("AL");
    expect(initials("  ")).toBe("?");
    expect(initials("Jan Maria Rokita")).toBe("JM");
    render(
      <div>
        <Avatar name="Marta Nowak" />
        <Avatar name="alice" size={28} decorative />
        <EnsoMark size={40} />
        <EnsoMark size={64} label="Plumb" />
      </div>,
    );
    expect(screen.getByRole("img", { name: "Marta Nowak" }).textContent).toBe("MN");
    expect(screen.getByText("AL").getAttribute("aria-hidden")).toBe("true");
    expect(screen.getByRole("img", { name: "Plumb" }).getAttribute("width")).toBe("64");
    expect(await axeViolations()).toEqual([]);
  });
});

describe("EmptyState and Skeleton", () => {
  it("shows the enso, check or custom visual with one sentence and one action", async () => {
    const { rerender } = render(<EmptyState title="Nothing here" description="Import a document." action={<Button>Import</Button>} />);
    expect(screen.getByRole("heading", { level: 2, name: "Nothing here" })).toBeTruthy();
    expect(document.querySelector("[data-enso]")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Import" })).toBeTruthy();
    expect(await axeViolations()).toEqual([]);
    rerender(<EmptyState title="You're all caught up" visual="check" headingLevel={3} />);
    expect(screen.getByRole("heading", { level: 3 })).toBeTruthy();
    expect(document.querySelector("[data-empty-check]")).not.toBeNull();
    rerender(<EmptyState title="Custom" visual={<span data-testid="custom" />} />);
    expect(screen.getByTestId("custom")).toBeTruthy();
    rerender(<EmptyState title="Bare" visual="none" />);
    expect(document.querySelector("[data-enso]")).toBeNull();
  });

  it("renders shimmer placeholders that announce loading once", async () => {
    render(
      <div>
        <Skeleton width={120} height={12} />
        <Skeleton radius="pill" className="w-full" />
        <Skeleton radius="full" width={30} height={30} />
        <Skeleton radius="card" width="50%" height={80} />
        <SkeletonList rows={3} />
      </div>,
    );
    const status = screen.getByRole("status", { name: "Loading" });
    expect(status.querySelectorAll("[data-skeleton]").length).toBeGreaterThanOrEqual(3);
    const first = document.querySelector("[data-skeleton]") as HTMLElement;
    expect(first.getAttribute("aria-hidden")).toBe("true");
    expect(first.style.width).toBe("120px");
    expect(classOf(first)).toContain("animate-shimmer");
    expect(await axeViolations()).toEqual([]);
  });
});
