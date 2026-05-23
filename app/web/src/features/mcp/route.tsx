import { Server } from "lucide-react";
import type { FeatureRoute } from "@/features/registry";
import { MCPServersPage } from "./MCPServersPage";

export const route: FeatureRoute = {
  path: "/mcp",
  label: "MCP Servers",
  icon: Server,
  order: 40,
  Component: MCPServersPage,
};
