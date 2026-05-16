/**
 * Projects management page
 */
import { useState } from 'react';
import { FolderOpen } from 'lucide-react';
import { useProjectContext } from '@/contexts/ProjectContext';
import { ProjectList } from './ProjectList';
import { ProjectDiscovery } from './ProjectDiscovery';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { RefreshButton } from '@/components/shared/RefreshButton';
import { PageHeader } from '@/components/layout/PageHeader';

export function ProjectsPage() {
  const { projects, loading, error, fetchProjects } = useProjectContext();
  const [showDiscovery, setShowDiscovery] = useState(false);

  return (
    <div className="space-y-4 md:space-y-6">
      <PageHeader
        title="Projects"
        description="Manage Claude Code project directories"
        icon={FolderOpen}
      >
        <RefreshButton onClick={fetchProjects} loading={loading} />
        <Button size="sm" onClick={() => setShowDiscovery(!showDiscovery)} className="h-8 md:h-10 text-xs md:text-sm">
          {showDiscovery ? 'Hide Discovery' : 'Discover Projects'}
        </Button>
      </PageHeader>

      {showDiscovery && (
        <ProjectDiscovery onProjectsDiscovered={() => {
          setShowDiscovery(false);
        }} />
      )}

      {error && (
        <Card className="border-destructive">
          <CardHeader>
            <CardTitle className="text-destructive">Error</CardTitle>
          </CardHeader>
          <CardContent>
            <p>{error}</p>
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader>
          <CardTitle>Tracked Projects</CardTitle>
          <CardDescription>
            {projects.length} project{projects.length !== 1 ? 's' : ''} tracked
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ProjectList projects={projects} loading={loading} />
        </CardContent>
      </Card>
    </div>
  );
}
