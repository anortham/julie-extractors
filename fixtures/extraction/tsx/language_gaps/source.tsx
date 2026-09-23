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

import { useNavigate } from "react-router-dom";

export function* steps(): Generator<number> {
  yield 1;
}

namespace Theme {
  export const primary = "blue";
}

export function SaveButton() {
  const navigate = useNavigate();
  const onClick = () => navigate("/saved");
  return <button onClick={onClick}>Save</button>;
}

const Card = class {
  render() {
    return <Badge label="card" />;
  }
};

export { Badge, Legacy as LegacyPage };
