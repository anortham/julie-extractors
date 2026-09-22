import React from "react";
import { UserCard } from "./UserCard";
import * as UI from "./ui";

/** Badge label. */
function Badge({ label }: { label: string }) {
  return <span>{label}</span>;
}

/** Profile page. */
export const Profile = () => (
  <UI.Panel title="Profile">
    <UserCard />
    <Badge label="new" />
    <div />
  </UI.Panel>
);

export default function App() {
  return <Profile />;
}

class Legacy extends React.Component {
  render() {
    return <Badge label="legacy" />;
  }
}
