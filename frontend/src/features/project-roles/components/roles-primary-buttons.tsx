import { IconPlus } from '@tabler/icons-react';

import { PermissionGuard } from '@/components/permission-guard';
import { Button } from '@/components/ui/button';

import { useRolesContext } from '../context/roles-context';
import * as m from '@/paraglide/messages';

export function RolesPrimaryButtons() {
  const { setIsCreateDialogOpen } = useRolesContext();

  return (
    <div className='flex items-center space-x-2'>
      {/* Create Role - requires write_roles permission */}
      <PermissionGuard requiredScope='write_roles'>
        <Button onClick={() => setIsCreateDialogOpen(true)}>
          <IconPlus className='mr-2 h-4 w-4' />
          {m["projectRoles.createRole"]()}
        </Button>
      </PermissionGuard>
    </div>
  );
}
