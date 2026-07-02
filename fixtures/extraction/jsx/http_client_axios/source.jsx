import axios from "axios";

export function UserList() {
  const load = () => axios.get("/api/users");
  const create = (payload) => axios("/api/users", { method: "post", data: payload });
  const dynamic = (url) => axios.get(url);
  return <button onClick={load}>load</button>;
}
