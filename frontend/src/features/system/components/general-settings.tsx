'use client';

import { Loader2, Save } from 'lucide-react';
import React, { useState } from 'react';

import { AutoCompleteSelect } from '@/components/auto-complete-select';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';

import { useSystemContext } from '../context/system-context';
import { currencyCodes } from '../data/currencies';
import {
  useGeneralSettings,
  useUpdateGeneralSettings,
  useUpdateUserAgentPassThroughSettings,
  useUserAgentPassThroughSettings,
} from '../data/system';
import { GMTTimeZoneOptions } from '../data/timezones';
import * as m from '@/paraglide/messages';
import { dynamicTranslation } from '@/lib/paraglide-helpers';

export function GeneralSettings() {
  const { data: settings, isLoading: isLoadingSettings } = useGeneralSettings();
  const { data: userAgentPassThroughSettings, isLoading: isLoadingUserAgentPassThroughSettings } = useUserAgentPassThroughSettings();
  const updateSettings = useUpdateGeneralSettings();
  const updateUserAgentPassThroughSettings = useUpdateUserAgentPassThroughSettings();
  const { isLoading, setIsLoading } = useSystemContext();

  const [currencyCode, setCurrencyCode] = useState('USD');
  const [timezone, setTimezone] = useState('UTC');
  const [passThroughUserAgent, setPassThroughUserAgent] = useState(false);

  const currencyItems = React.useMemo(
    () =>
      currencyCodes.map((code) => ({
        value: code,
        label: dynamicTranslation(`currencies.${code}`),
      })),
    []
  );

  const timezoneItems = React.useMemo(() => GMTTimeZoneOptions, []);

  // Update local state when settings are loaded
  React.useEffect(() => {
    if (settings) {
      setCurrencyCode(settings.currencyCode || 'USD');
      setTimezone(settings.timezone || 'UTC');
    }
  }, [settings]);

  React.useEffect(() => {
    if (userAgentPassThroughSettings) {
      setPassThroughUserAgent(userAgentPassThroughSettings.enabled);
    }
  }, [userAgentPassThroughSettings]);

  const handleSave = async () => {
    setIsLoading(true);
    try {
      await updateSettings.mutateAsync({
        currencyCode: currencyCode.trim(),
        timezone: timezone.trim(),
      });

      if (userAgentPassThroughSettings && userAgentPassThroughSettings.enabled !== passThroughUserAgent) {
        await updateUserAgentPassThroughSettings.mutateAsync({
          enabled: passThroughUserAgent,
        });
      }
    } finally {
      setIsLoading(false);
    }
  };

  const hasChanges = settings
    ? settings.currencyCode !== currencyCode ||
      settings.timezone !== timezone ||
      !!userAgentPassThroughSettings && userAgentPassThroughSettings.enabled !== passThroughUserAgent
    : false;

  if (isLoadingSettings || isLoadingUserAgentPassThroughSettings) {
    return (
      <div className='flex h-32 items-center justify-center'>
        <Loader2 className='h-6 w-6 animate-spin' />
        <span className='text-muted-foreground ml-2'>{m["common.loading"]()}</span>
      </div>
    );
  }

  return (
    <div className='space-y-6'>
      <Card>
        <CardHeader>
          <CardTitle>{m["system.general.title"]()}</CardTitle>
          <CardDescription>{m["system.general.description"]()}</CardDescription>
        </CardHeader>
        <CardContent className='space-y-6'>
          <div className='space-y-2'>
            <Label htmlFor='currency-code'>{m["system.general.currencyCode.label"]()}</Label>
            <div className='max-w-md'>
              <AutoCompleteSelect
                selectedValue={currencyCode}
                onSelectedValueChange={setCurrencyCode}
                items={currencyItems}
                placeholder={m["system.general.currencyCode.placeholder"]()}
                isLoading={isLoadingSettings}
              />
            </div>
            <div className='text-muted-foreground text-sm'>{m["system.general.currencyCode.description"]()}</div>
          </div>

          <div className='space-y-2'>
            <Label htmlFor='timezone'>{m["system.general.timezone.label"]()}</Label>
            <div className='max-w-md'>
              <AutoCompleteSelect
                selectedValue={timezone}
                onSelectedValueChange={setTimezone}
                items={timezoneItems}
                placeholder={m["system.general.timezone.placeholder"]()}
                isLoading={isLoadingSettings}
              />
            </div>
            <div className='text-muted-foreground text-sm'>{m["system.general.timezone.description"]()}</div>
          </div>

          <div className='flex items-center justify-between rounded-xl border p-4'>
            <div className='space-y-1 pr-4'>
              <Label htmlFor='user-agent-pass-through'>Pass through User-Agent</Label>
              <div className='text-muted-foreground text-sm'>
                Forward the original client User-Agent header to upstream providers when supported.
              </div>
            </div>
            <Switch
              id='user-agent-pass-through'
              checked={passThroughUserAgent}
              onCheckedChange={setPassThroughUserAgent}
              disabled={isLoading || updateSettings.isPending || updateUserAgentPassThroughSettings.isPending}
            />
          </div>
        </CardContent>
      </Card>

      {hasChanges && (
        <div className='flex justify-end'>
          <Button
            onClick={handleSave}
            disabled={isLoading || updateSettings.isPending || updateUserAgentPassThroughSettings.isPending}
            className='min-w-[100px]'
          >
            {isLoading || updateSettings.isPending || updateUserAgentPassThroughSettings.isPending ? (
              <>
                <Loader2 className='mr-2 h-4 w-4 animate-spin' />
                {m["system.buttons.saving"]()}
              </>
            ) : (
              <>
                <Save className='mr-2 h-4 w-4' />
                {m["system.buttons.save"]()}
              </>
            )}
          </Button>
        </div>
      )}
    </div>
  );
}
