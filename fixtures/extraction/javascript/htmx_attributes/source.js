export const NavButtons = () => (
  <nav>
    <button hx-get="/home">Home</button>
    <button data-hx-post="/logout">Logout</button>
  </nav>
);
