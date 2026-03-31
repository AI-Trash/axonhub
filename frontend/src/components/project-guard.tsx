import { IconFolderOff, IconFolderPlus } from '@tabler/icons-react';
import { useRouter } from '@tanstack/react-router';
import { useEffect } from 'react';

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { useMyProjects } from '@/features/projects/data/projects';
import { useSelectedProjectId } from '@/stores/projectStore';
import * as m from '@/paraglide/messages';

interface ProjectGuardProps {
  children: React.ReactNode;
  fallbackPath?: string;
  showNoProjectPage?: boolean;
}

export function ProjectGuard({ children, fallbackPath = '/projects', showNoProjectPage = true }: ProjectGuardProps) {
  const router = useRouter();
  const selectedProjectId = useSelectedProjectId();
  const { data: myProjects, isLoading } = useMyProjects();

  // 检查是否有选中的项目
  const hasSelectedProject = !!selectedProjectId;

  // 检查用户是否有任何项目
  const hasAnyProjects = !isLoading && myProjects && myProjects.length > 0;

  useEffect(() => {
    // 如果没有选中项目且不显示提示页面，则重定向
    if (!hasSelectedProject && !showNoProjectPage && !isLoading) {
      router.navigate({ to: fallbackPath });
    }
  }, [hasSelectedProject, showNoProjectPage, fallbackPath, router, isLoading]);

  // 加载中时不显示任何内容
  if (isLoading) {
    return null;
  }

  // 如果没有选中项目
  if (!hasSelectedProject) {
    if (showNoProjectPage) {
      return <NoProjectPage hasAnyProjects={!!hasAnyProjects} onGoToProjects={() => router.navigate({ to: fallbackPath })} />;
    }
    return null; // 重定向中，不显示任何内容
  }

  return <>{children}</>;
}

function NoProjectPage({ hasAnyProjects, onGoToProjects }: { hasAnyProjects: boolean; onGoToProjects: () => void }) {
  return (
    <div className='flex h-screen items-center justify-center'>
      <div className='max-w-md text-center'>
        <div className='mb-6'>
          <IconFolderOff className='mx-auto h-16 w-16 text-orange-500' />
        </div>

        <Alert className='mb-6'>
          <IconFolderOff className='h-4 w-4' />
          <AlertTitle>{m["common.projectGuard.noProjectSelected"]()}</AlertTitle>
          <AlertDescription>
            {hasAnyProjects ? m["common.projectGuard.pleaseSelectProject"]() : m["common.projectGuard.pleaseJoinOrCreateProject"]()}
          </AlertDescription>
        </Alert>

        {/* <Button onClick={onGoToProjects} className="gap-2">
          <IconFolderPlus className="h-4 w-4" />
          {hasAnyProjects 
            ? m["common.projectGuard.goToProjects"]()
            : m["common.projectGuard.createOrJoinProject"]()
          }
        </Button> */}
      </div>
    </div>
  );
}
