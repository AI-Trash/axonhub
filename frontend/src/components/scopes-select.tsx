import { Check, ChevronsUpDown } from 'lucide-react';
import { useState } from 'react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem } from '@/components/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { useAllScopes } from '@/gql/scopes';
import { filterGrantableScopes } from '@/lib/permission-utils';
import { cn } from '@/lib/utils';
import { useAuthStore } from '@/stores/authStore';
import { useSelectedProjectId } from '@/stores/projectStore';
import * as m from '@/paraglide/messages';
import { dynamicTranslation } from '@/lib/paraglide-helpers';

interface ScopesSelectProps {
  value: string[];
  onChange: (value: string[]) => void;
  portalContainer?: HTMLElement | null;
  level?: 'system' | 'project';
  enablePermissionFilter?: boolean;
}

export function ScopesSelect({ value, onChange, portalContainer, level = 'project', enablePermissionFilter = false }: ScopesSelectProps) {
  const [open, setOpen] = useState(false);
  const currentUser = useAuthStore((state) => state.auth.user);
  const selectedProjectId = useSelectedProjectId();
  const { data: allScopes } = useAllScopes(level);

  let filteredScopes = allScopes || [];

  if (enablePermissionFilter && currentUser) {
    const allScopeValues = allScopes?.map((s) => s.scope) || [];
    const grantableScopes = filterGrantableScopes(currentUser, allScopeValues, selectedProjectId);
    filteredScopes = allScopes?.filter((s) => grantableScopes.includes(s.scope)) || [];
  }

  const handleSelect = (scopeValue: string) => {
    const newValue = value.includes(scopeValue) ? value.filter((v) => v !== scopeValue) : [...value, scopeValue];
    onChange(newValue);
  };

  const handleRemove = (scopeValue: string) => {
    onChange(value.filter((v) => v !== scopeValue));
  };

  return (
    <div className='space-y-2'>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button variant='outline' role='combobox' aria-expanded={open} className='w-full justify-between'>
            {value.length > 0 ? m["scopes.select.selectedCount"]({ count: value.length }) : m["scopes.select.selectPlaceholder"]()}
            <ChevronsUpDown className='ml-2 h-4 w-4 shrink-0 opacity-50' />
          </Button>
        </PopoverTrigger>
        <PopoverContent className='w-full p-0' align='start' container={portalContainer}>
          <Command>
            <CommandInput placeholder={m["scopes.select.searchPlaceholder"]()} />
            <CommandEmpty>{m["scopes.select.noResults"]()}</CommandEmpty>
            <CommandGroup className='max-h-64 overflow-auto'>
              {filteredScopes.map((scope) => (
                <CommandItem key={scope.scope} value={scope.scope} onSelect={() => handleSelect(scope.scope)}>
                  <Check className={cn('mr-2 h-4 w-4', value.includes(scope.scope) ? 'opacity-100' : 'opacity-0')} />
                  <div className='flex flex-col'>
                    <span>{scope.scope}</span>
                    <span className='text-muted-foreground text-xs'>{dynamicTranslation(`scopes.${scope.scope}`)}</span>
                  </div>
                </CommandItem>
              ))}
            </CommandGroup>
          </Command>
        </PopoverContent>
      </Popover>

      {value.length > 0 && (
        <div className='flex flex-wrap gap-2'>
          {value.map((scopeValue) => {
            const scopeInfo = allScopes?.find((s) => s.scope === scopeValue);
            return (
              <Badge key={scopeValue} variant='secondary' className='cursor-pointer' onClick={() => handleRemove(scopeValue)}>
                {scopeInfo?.scope || scopeValue}
                <span className='ml-1 text-xs'>×</span>
              </Badge>
            );
          })}
        </div>
      )}
    </div>
  );
}
