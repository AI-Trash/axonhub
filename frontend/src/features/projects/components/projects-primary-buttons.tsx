import { IconPlus } from '@tabler/icons-react';

import { PermissionGuard } from '@/components/permission-guard';
import { Button } from '@/components/ui/button';

import { useProjectsContext } from '../context/projects-context';
import * as m from '@/paraglide/messages';

export function ProjectsPrimaryButtons() {
  const { setIsCreateDialogOpen } = useProjectsContext();

  return (
    <div className='flex items-center space-x-2'>
      {/* Create Project - requires write_projects permission */}
      <PermissionGuard requiredScope='write_projects'>
        <Button onClick={() => setIsCreateDialogOpen(true)}>
          <IconPlus className='mr-2 h-4 w-4' />
          {m["projects.createProject"]()}
        </Button>
      </PermissionGuard>
    </div>
  );
}
