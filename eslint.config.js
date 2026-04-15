// Project uses tsc for type checks; ESLint is intentionally not enforced.
// Flat-config globally ignoring the entire tree so tools silently no-op.
export default [
  {
    ignores: ["**/*"],
  },
];
