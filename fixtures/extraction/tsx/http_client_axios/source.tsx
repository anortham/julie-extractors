import axios from "axios";

interface User {
  id: number;
}

export function UserList() {
  const load = () => axios.get<User[]>("/api/users");
  const create = (payload: User) => axios("/api/users", { method: "post", data: payload });
  const dynamic = (url: string) => axios.get(url);
  return <button onClick={load}>load</button>;
}
