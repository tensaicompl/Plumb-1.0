import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

// Attributes whose literal values are shown to users or assistive technology.
const VISIBLE_ATTRIBUTES =
  "/^(aria-label|aria-description|aria-placeholder|aria-roledescription|aria-valuetext|title|placeholder|alt|label)$/";

// User-facing copy comes from src/i18n catalogues, never from JSX literals. Implementation-only
// strings (className, id, route paths, data-* attributes, technical values) stay allowed.
const hardCodedCopy = [
  "error",
  {
    selector: "JSXText[value=/\\S/]",
    message: "User-facing text must come from the i18n catalogue (src/i18n).",
  },
  {
    selector: "JSXExpressionContainer > Literal[value=/\\S/]",
    message: "User-facing text must come from the i18n catalogue (src/i18n).",
  },
  {
    selector: "JSXExpressionContainer > TemplateLiteral",
    message: "User-facing text must come from the i18n catalogue (src/i18n).",
  },
  {
    selector: `JSXAttribute[name.name=${VISIBLE_ATTRIBUTES}] > Literal`,
    message: "User-facing attribute text must come from the i18n catalogue (src/i18n).",
  },
  {
    selector: `JSXAttribute[name.name=${VISIBLE_ATTRIBUTES}] > JSXExpressionContainer > Literal`,
    message: "User-facing attribute text must come from the i18n catalogue (src/i18n).",
  },
];

export default tseslint.config(
  {
    ignores: ["dist/**", "node_modules/**", "coverage/**", "playwright-report/**", "test-results/**", "src/api/schema.d.ts"],
  },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [tseslint.configs.strict],
    plugins: {
      "react-hooks": reactHooks,
      "jsx-a11y": jsxA11y,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      ...jsxA11y.flatConfigs.recommended.rules,
    },
  },
  {
    files: ["src/**/*.tsx"],
    ignores: ["**/*.test.*", "src/test/**", "src/i18n/**"],
    rules: {
      "no-restricted-syntax": hardCodedCopy,
    },
  },
);
