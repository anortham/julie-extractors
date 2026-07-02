import axios from "axios";

interface User {
  id: number;
}

export async function loadUsers(): Promise<User[]> {
  const response = await axios.get<User[]>("/api/users");
  return response.data;
}

export async function createUser(payload: User): Promise<void> {
  await axios("/api/users", { method: "post", data: payload });
}

export function dynamicRequest(url: string): Promise<unknown> {
  return axios.get(url);
}
