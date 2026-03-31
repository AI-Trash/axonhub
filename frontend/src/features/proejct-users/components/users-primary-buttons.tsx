import { IconUserPlus } from '@tabler/icons-react';

import { PermissionGuard } from '@/components/permission-guard';
import { Button } from '@/components/ui/button';

import { useUsers } from '../context/users-context';
import * as m from '@/paraglide/messages';

export function UsersPrimaryButtons() {
  const { setOpen } = useUsers();
  return (
    <div className='flex gap-2'>
      {/* Add User - requires system-level read_users and any-level write_users */}
      <PermissionGuard requiredSystemScope='read_users' requiredScope='write_users'>
        <Button className='space-x-1' onClick={() => setOpen('add')}>
          <span>{m["users.addUser"]()}</span> <IconUserPlus size={18} />
        </Button>
      </PermissionGuard>
    </div>
  );
}
