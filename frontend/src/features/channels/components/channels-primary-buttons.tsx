import { IconPlus, IconUpload, IconArrowsSort, IconSettings, IconScale } from '@tabler/icons-react';
import { useNavigate } from '@tanstack/react-router';

import { PermissionGuard } from '@/components/permission-guard';
import { Button } from '@/components/ui/button';

import { useChannels } from '../context/channels-context';
import * as m from '@/paraglide/messages';

export function ChannelsPrimaryButtons() {
  const navigate = useNavigate();
  const { setOpen } = useChannels();

  return (
    <div className='flex gap-2 overflow-x-auto md:overflow-x-visible'>
      <PermissionGuard requiredScope='read_system'>
        {/* Load Balancing Strategy - navigate to system retry configuration */}
        <Button variant='outline' className='shrink-0 space-x-1' onClick={() => navigate({ to: '/system', search: { tab: 'retry' } })}>
          <span>{m["channels.loadBalancingStrategy"]()}</span> <IconScale size={18} />
        </Button>
      </PermissionGuard>

      <PermissionGuard requiredScope='write_channels'>
        <>
          {/* Settings - requires write_channels permission */}
          <Button variant='outline' className='shrink-0 space-x-1' onClick={() => setOpen('channelSettings')}>
            <span>{m["channels.actions.settings"]()}</span> <IconSettings size={18} />
          </Button>

          {/* Bulk Import - requires write_channels permission */}
          <Button variant='outline' className='shrink-0 space-x-1' onClick={() => setOpen('bulkImport')}>
            <span>{m["channels.importChannels"]()}</span> <IconUpload size={18} />
          </Button>

          {/* Bulk Ordering - requires write_channels permission */}
          <Button variant='outline' className='shrink-0 space-x-1' onClick={() => setOpen('bulkOrdering')}>
            <span>{m["channels.orderChannels"]()}</span> <IconArrowsSort size={18} />
          </Button>

          {/* Add Channel - requires write_channels permission */}
          <Button className='shrink-0 space-x-1' onClick={() => setOpen('add')} data-testid='add-channel-button'>
            <span>{m["channels.addChannel"]()}</span> <IconPlus size={18} />
          </Button>
        </>
      </PermissionGuard>
    </div>
  );
}
