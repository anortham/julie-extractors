import { Link as RouterLink, Route, createBrowserRouter } from "react-router-dom";
import NextLink from "next/link";

export async function load() {
    return await fetch("/api");
}

const routes = [
    { path: "/dashboard", Component: Dashboard, id: "dashboard" },
    { index: true, Component: Home }
];

export const router = createBrowserRouter(routes);

export function Navigation() {
    return (
        <>
            <RouterLink to="/dashboard">Dashboard</RouterLink>
            <Route path="/settings" Component={Settings} />
            <NextLink href="/about">About</NextLink>
        </>
    );
}
