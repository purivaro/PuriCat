import type { Account, AccountRef, Engine, UserSpec, UserStep, UserTemplate } from "../../lib/types";

/** The account as people write it, and as the drop form asks for it to be typed. */
export function accountLabel(account: AccountRef): string {
  return account.host ? `${account.name}@${account.host}` : account.name;
}

export const sameAccount = (a: AccountRef, b: AccountRef) => a.name === b.name && (a.host ?? null) === (b.host ?? null);

/** Accounts whose name or host holds the text typed in the search box, whatever the case. */
export function filterAccounts<T extends AccountRef>(accounts: T[], query: string): T[] {
  const wanted = query.trim().toLocaleLowerCase();
  if (!wanted) return accounts;
  return accounts.filter((a) => accountLabel(a).toLocaleLowerCase().includes(wanted));
}

/** The accounts the forms leave alone (the backend refuses them too). */
export const untouchable = (account: Account) => account.you || account.system || account.superuser;

/** What the "New user" form holds. */
export interface UserDraft {
  name: string;
  /** MySQL only: `%`, `localhost`, or what was typed for "another host". */
  host: string;
  template: UserTemplate;
  database: string;
  password: string;
}

/** The form's choices as the backend takes them: only what belongs to the engine and the template. */
export function userSpec(engine: Engine, draft: UserDraft): UserSpec {
  return {
    account: { name: draft.name, host: engine === "mysql" ? draft.host : null },
    template: draft.template,
    database: draft.template === "none" ? null : draft.database || null,
    password: draft.password,
  };
}

const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/**
 * A password of letters and digits from the system's random source: nothing in it needs
 * escaping in a statement, a shell or a `.env` file. Bytes that would favour the first
 * letters of the alphabet are thrown away, so every character is equally likely.
 */
export function generatePassword(
  length = 24,
  random: (bytes: Uint8Array<ArrayBuffer>) => void = (bytes) => void crypto.getRandomValues(bytes),
): string {
  const fair = 256 - (256 % ALPHABET.length);
  let out = "";
  while (out.length < length) {
    const bytes = new Uint8Array(length * 2);
    random(bytes);
    for (const byte of bytes) {
      if (byte < fair && out.length < length) out += ALPHABET[byte % ALPHABET.length];
    }
  }
  return out;
}

/** The steps of a preview in the groups they are shown in: the server's, then each database's. */
export function groupSteps(steps: UserStep[]): { database: string | null; sql: string[] }[] {
  const groups: { database: string | null; sql: string[] }[] = [];
  for (const step of steps) {
    const last = groups[groups.length - 1];
    if (last && last.database === step.database) last.sql.push(step.sql);
    else groups.push({ database: step.database, sql: [step.sql] });
  }
  return groups;
}
