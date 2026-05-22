import { Webhook } from "lucide-react";
import type { FeatureRoute } from "@/features/registry";
import { HooksPage } from "./HooksPage";

export const route: FeatureRoute = {
  path: "/hooks",
  label: "Hooks",
  icon: Webhook,
  order: 70,
  Component: HooksPage,
};
