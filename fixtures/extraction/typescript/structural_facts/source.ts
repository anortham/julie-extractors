import { createBrowserRouter } from "react-router-dom";
import { createRouter, createWebHistory } from "vue-router";

export async function load(): Promise<Response> {
    return await fetch("/api");
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
