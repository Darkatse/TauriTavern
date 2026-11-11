import jsxA11y from 'eslint-plugin-jsx-a11y';
import reactHooks from 'eslint-plugin-react-hooks';
import tseslint from 'typescript-eslint';

const ownedUiFiles = [
  'packages/app-use/src/**/*.{ts,tsx}',
  'src/scripts/extensions/agent-system/src/**/*.{ts,tsx}',
  'src/scripts/extensions/in-app-agent/src/**/*.{ts,tsx}',
  'src/scripts/extensions/mcp-manager/src/**/*.{ts,tsx}',
  'src/scripts/tauri/setting/**/*.{ts,tsx}',
];

export default tseslint.config(
  {
    files: ownedUiFiles,
    extends: [
      ...tseslint.configs.recommendedTypeChecked,
      reactHooks.configs.flat.recommended,
      jsxA11y.flatConfigs.recommended,
    ],
    languageOptions: {
      parserOptions: {
        project: './tsconfig.ui.json',
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      'max-lines': ['error', 500],
      // Switch copy is nested under label > span > strong/small.
      'jsx-a11y/label-has-associated-control': ['error', { depth: 3 }],
      // A focusable separator is the keyboard-operated window-splitter pattern.
      'jsx-a11y/no-interactive-element-to-noninteractive-role': ['error', {
        button: ['separator'],
      }],
      'no-restricted-imports': ['error', {
        paths: [{ name: 'vue', message: 'First-party typed UI uses React.' }],
        patterns: [{ group: ['vue/*'], message: 'First-party typed UI uses React.' }],
      }],
    },
  },
  {
    files: ['packages/app-use/src/**/*.{ts,tsx}'],
    languageOptions: {
      parserOptions: { project: './packages/app-use/tsconfig.json' },
    },
  },
  {
    files: ['src/scripts/extensions/mcp-manager/src/test-call-dialog.tsx'],
    rules: { 'max-lines': ['error', 613] },
  },
);
