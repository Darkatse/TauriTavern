import jsxA11y from 'eslint-plugin-jsx-a11y';
import reactHooks from 'eslint-plugin-react-hooks';
import tseslint from 'typescript-eslint';

export default tseslint.config({
  files: ['src/scripts/extensions/**/*.{ts,tsx}'],
  extends: [
    ...tseslint.configs.recommendedTypeChecked,
    reactHooks.configs.flat.recommended,
    jsxA11y.flatConfigs.recommended,
  ],
  languageOptions: {
    parserOptions: {
      projectService: true,
      tsconfigRootDir: import.meta.dirname,
    },
  },
});
