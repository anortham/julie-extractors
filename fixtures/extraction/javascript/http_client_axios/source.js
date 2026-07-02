import axios from "axios";

export async function loadUsers() {
  return axios.get("/api/users");
}

export async function createUser(payload) {
  return axios("/api/users", { method: "post", data: payload });
}

export function dynamicRequest(url) {
  return axios.get(url);
}
