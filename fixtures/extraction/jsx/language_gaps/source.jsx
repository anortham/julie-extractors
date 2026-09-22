import TodoItem from "./TodoItem";
import * as UI from "../ui";

/** Empty list notice. */
function EmptyState({ message }) {
  return <p>{message}</p>;
}

/** Item list. */
export function List({ items }) {
  if (!items.length) return <EmptyState message="none" />;
  return (
    <UI.Panel>
      {items.map((item) => (
        <TodoItem key={item.id} todo={item} />
      ))}
    </UI.Panel>
  );
}
