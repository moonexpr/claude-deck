import { useState, useEffect, useCallback } from 'react'
import { Settings, Eye, Edit, Shield } from 'lucide-react'
import type { ConfigFileListResponse, ConfigValue } from '@/types/config'
import { RefreshButton } from '@/components/shared/RefreshButton'
import { PageHeader } from '@/components/layout/PageHeader'
import { ConfigFileList } from './ConfigFileList'
import { ConfigFileViewer } from './ConfigFileViewer'
import { SettingsEditor } from './settings'
import { ScopeResolver } from './ScopeResolver'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { apiClient, buildEndpoint } from '@/lib/api'
import { useProjectStore } from '@/stores/useProjectStore'
import { toast } from 'sonner'

export function ConfigViewerPage() {
  const activeProject = useProjectStore((s) => s.activeProject)
  const [data, setData] = useState<ConfigFileListResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [activeTab, setActiveTab] = useState<'editor' | 'scopes' | 'viewer'>('editor')

  const fetchData = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const endpoint = buildEndpoint('config/files', { project_path: activeProject?.path })
      const response = await apiClient<ConfigFileListResponse>(endpoint)
      setData(response)
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to load config files'
      setError(message)
      toast.error(message)
    } finally {
      setLoading(false)
    }
  }, [activeProject?.path])

  useEffect(() => {
    fetchData()
  }, [fetchData])

  const handleOverrideInLocal = async (key: string, value: ConfigValue) => {
    const parts = key.split('.')
    const settings: Record<string, ConfigValue> = {}
    let current: Record<string, ConfigValue> = settings
    for (let i = 0; i < parts.length - 1; i++) {
      const nested: Record<string, ConfigValue> = {}
      current[parts[i]] = nested
      current = nested
    }
    current[parts[parts.length - 1]] = value

    try {
      await apiClient('config/settings', {
        method: 'PUT',
        body: JSON.stringify({
          scope: 'local',
          settings,
          project_path: activeProject?.path
        })
      })
      toast.success(`Setting "${key}" copied to local scope`)
      fetchData()
    } catch {
      toast.error('Failed to copy setting to local scope')
    }
  }

  return (
    <div className="space-y-4 md:space-y-6 h-full flex flex-col">
      <PageHeader
        title="Configuration"
        description="View and edit Claude Code configuration"
        icon={Settings}
      >
        <RefreshButton onClick={fetchData} loading={loading} />
      </PageHeader>

      <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as typeof activeTab)} className="flex-1 flex flex-col overflow-hidden">
        <div className="overflow-x-auto pb-2 -mb-2">
          <TabsList className="w-full justify-start sm:justify-center">
            <TabsTrigger value="editor" className="flex items-center gap-2">
              <Edit className="h-4 w-4" />
              Settings Editor
            </TabsTrigger>
            <TabsTrigger value="scopes" className="flex items-center gap-2">
              <Shield className="h-4 w-4" />
              Scope Resolver
            </TabsTrigger>
            <TabsTrigger value="viewer" className="flex items-center gap-2">
              <Eye className="h-4 w-4" />
              Raw Viewer
            </TabsTrigger>
          </TabsList>
        </div>

        <TabsContent value="editor" className="flex-1 overflow-auto mt-4">
          <SettingsEditor onSave={fetchData} />
        </TabsContent>

        <TabsContent value="scopes" className="flex-1 overflow-auto mt-4">
          <ScopeResolver onOverride={activeProject ? handleOverrideInLocal : undefined} />
        </TabsContent>

        <TabsContent value="viewer" className="flex-1 overflow-hidden mt-4">
          {error && (
            <Card className="border-destructive mb-4">
              <CardHeader className="p-4">
                <CardTitle className="text-destructive text-lg">Error</CardTitle>
              </CardHeader>
              <CardContent className="px-4 pb-4">
                <p className="text-sm">{error}</p>
              </CardContent>
            </Card>
          )}

          <div className="flex flex-col lg:grid lg:grid-cols-12 gap-6 h-full overflow-hidden">
            <div className="lg:col-span-4 overflow-y-auto">
              <Card className="h-auto lg:h-full">
                <CardHeader className="p-4">
                  <CardTitle className="text-lg">Config Files</CardTitle>
                </CardHeader>
                <CardContent className="px-4 pb-4">
                  {loading && !data && (
                    <p className="text-sm text-muted-foreground">Loading files...</p>
                  )}
                  {data && (
                    <ConfigFileList
                      files={data.files}
                      selectedFile={selectedFile}
                      onSelectFile={setSelectedFile}
                    />
                  )}
                </CardContent>
              </Card>
            </div>

            <div className="lg:col-span-8 overflow-y-auto min-h-[400px]">
              <ConfigFileViewer filePath={selectedFile} />
            </div>
          </div>
        </TabsContent>
      </Tabs>
    </div>
  )
}
