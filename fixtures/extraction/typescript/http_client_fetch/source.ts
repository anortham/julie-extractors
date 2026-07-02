export async function loadUsers(): Promise<Response> {
  return fetch("/api/users");
}

export async function createUser(payload: BodyInit): Promise<Response> {
  return fetch("/api/users", { method: "POST", body: payload });
}

export function dynamicFetch(url: string): Promise<Response> {
  return fetch(url);
}
