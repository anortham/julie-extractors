export async function loadUsers() {
  return fetch("/api/users");
}

export async function createUser(payload) {
  return fetch("/api/users", { method: "POST", body: payload });
}

export function dynamicFetch(url) {
  return fetch(url);
}
