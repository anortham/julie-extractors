export function TodoList({ dynamicUrl }) {
  return (
    <div>
      <button hx-post="/todos" hx-target="#list">Add</button>
      <button data-hx-get="/todos">Refresh</button>
      <button hx-delete={dynamicUrl}>Remove</button>
    </div>
  );
}
