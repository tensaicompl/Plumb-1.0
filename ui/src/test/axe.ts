import axe from "axe-core";

/**
 * The axe violations of `root` (jsdom). Colour contrast needs real layout and fonts, so it is
 * checked in the Playwright pilot run instead; every other WCAG A/AA rule runs here.
 */
export async function axeViolations(root: Element = document.body): Promise<string[]> {
  const results = await axe.run(root, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] },
    rules: {
      "color-contrast": { enabled: false },
      // Components are rendered in isolation, outside a page with landmarks and a heading.
      region: { enabled: false },
      "page-has-heading-one": { enabled: false },
      "landmark-one-main": { enabled: false },
    },
  });
  return results.violations.map((v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(" ")).join(", ")})`);
}
