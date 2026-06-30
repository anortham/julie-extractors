import { createBrowserRouter } from "react-router-dom";

export async function load(): Promise<Response> {
    return await fetch("/api");
}

const routes = [
    { path: "/dashboard", Component: Dashboard, id: "dashboard" },
    { index: true, Component: Home }
];

export const router = createBrowserRouter(routes);
