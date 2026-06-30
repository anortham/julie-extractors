import { Link as RouterLink, Route, createBrowserRouter } from "react-router-dom";
import NextLink from "next/link";

export async function View() {
    const data = await load();
    return (
        <>
            <div>{data}</div>
            <RouterLink to="/dashboard">Dashboard</RouterLink>
            <Route path="/settings" Component={Settings} />
            <NextLink href="/about">About</NextLink>
        </>
    );
}

const routes = [
    { path: "/dashboard", Component: Dashboard, id: "dashboard" },
    { index: true, Component: Home }
];

export const router = createBrowserRouter(routes);
