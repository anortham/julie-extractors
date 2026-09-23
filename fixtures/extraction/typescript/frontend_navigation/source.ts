import { NgModule } from "@angular/core";
import { Routes, RouterModule } from "@angular/router";
import { useNavigate } from "react-router-dom";
import { useRouter } from "next/navigation";
import { UsersComponent } from "./users.component";
import { ShellComponent } from "./shell.component";

const routes: Routes = [
  { path: "users", component: UsersComponent },
  {
    path: "admin",
    loadChildren: () => import("./admin/admin.module").then((m) => m.AdminModule),
  },
  {
    path: "app",
    component: ShellComponent,
    children: [
      { path: "", redirectTo: "home", pathMatch: "full" },
      {
        path: "profile/:id",
        loadComponent: () => import("./profile.component").then((m) => m.ProfileComponent),
      },
    ],
  },
];

@NgModule({ imports: [RouterModule.forRoot(routes)] })
export class AppRoutingModule {}

export function useSaveRedirect() {
  const navigate = useNavigate();
  const router = useRouter();
  return (done: boolean) => {
    if (done) {
      navigate("/settings");
    } else {
      router.push("/users/1");
    }
  };
}
