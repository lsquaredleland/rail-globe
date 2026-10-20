/** `.catch(ignoreError('what failed'))`: a failure that needs no handling (a side file that is missing, a clipboard that is blocked). Logged in dev, silent in production. */
export const ignoreError =
  (what: string) =>
  (e: unknown): void => {
    if (import.meta.env.DEV) console.debug(`[rail-globe] ${what}`, e)
  }
