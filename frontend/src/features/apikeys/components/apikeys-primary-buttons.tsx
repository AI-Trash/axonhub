import { IconPlus } from '@tabler/icons-react';

import { Button } from '@/components/ui/button';

import { useApiKeysContext } from '../context/apikeys-context';
import * as m from '@/paraglide/messages';

export function ApiKeysPrimaryButtons() {
  const { openDialog } = useApiKeysContext();

  return (
    <div className='flex gap-2'>
      <Button onClick={() => openDialog('create')} size='sm'>
        <IconPlus className='mr-2 h-4 w-4' />
        {m["apikeys.createApiKey"]()}
      </Button>
    </div>
  );
}
