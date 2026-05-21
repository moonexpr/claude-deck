import { BrowserRouter, Routes, Route } from "react-router-dom";
import { MainLayout } from "@/components/layout/MainLayout";
import { HomePage } from "@/pages/HomePage";
import { CodeEditorDemoPage } from "@/pages/CodeEditorDemoPage";

export function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route element={<MainLayout />}>
          <Route index element={<HomePage />} />
          <Route path="dev/editor" element={<CodeEditorDemoPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
