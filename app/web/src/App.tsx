import { BrowserRouter, Routes, Route } from "react-router-dom";
import { Toaster } from "sonner";
import { MainLayout } from "@/components/layout/MainLayout";
import { featureRoutes } from "@/features/registry";

export function App() {
  return (
    <BrowserRouter>
      <Toaster richColors position="top-right" />
      <Routes>
        <Route element={<MainLayout />}>
          {featureRoutes.map((route) => (
            <Route
              key={route.path}
              path={route.path === '/' ? undefined : route.path}
              index={route.path === '/'}
              element={<route.Component />}
            />
          ))}
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
