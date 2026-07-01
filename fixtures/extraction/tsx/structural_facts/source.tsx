import { Link as RouterLink, Route, createBrowserRouter } from "react-router-dom";
import NextLink from "next/link";
import { createRouter, createWebHistory } from "vue-router";

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

const vueRoutes = [
    { path: "/vue-dashboard", component: VueDashboard }
];

export const vueRouter = createRouter({
    history: createWebHistory(),
    routes: vueRoutes
});
