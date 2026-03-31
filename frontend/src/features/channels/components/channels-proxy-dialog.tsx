'use client';

import { zodResolver } from '@hookform/resolvers/zod';
import { IconPlayerPlay } from '@tabler/icons-react';
import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { toast } from 'sonner';
import { z } from 'zod';

import LongText from '@/components/long-text';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { useProxyPresets, useSaveProxyPreset } from '@/features/system/data/system';

import { useUpdateChannel, useTestChannel } from '../data/channels';
import { Channel } from '../data/schema';
import { ErrorDisplay } from '../utils/error-formatter';
import { mergeChannelSettingsForUpdate } from '../utils/merge';
import * as m from '@/paraglide/messages';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentRow: Channel;
}

// Proxy type enum
export enum ProxyType {
  DISABLED = 'disabled',
  ENVIRONMENT = 'environment',
  URL = 'url',
}

// Proxy configuration schema
const proxyConfigSchema = z
  .object({
    type: z.nativeEnum(ProxyType),
    url: z.string().optional(),
    username: z.string().optional(),
    password: z.string().optional(),
  })
  .refine(
    (data) => {
      // If type is URL, url field is required
      if (data.type === ProxyType.URL) {
        return !!data.url && data.url.trim() !== '';
      }
      return true;
    },
    {
      message: 'Proxy URL is required when type is URL',
      path: ['url'],
    }
  );

type ProxyConfig = z.infer<typeof proxyConfigSchema>;

export function ChannelsProxyDialog({ open, onOpenChange, currentRow }: Props) {
  const updateChannel = useUpdateChannel();
  const testChannel = useTestChannel();
  const [isTesting, setIsTesting] = useState(false);
  const { data: proxyPresets = [] } = useProxyPresets();
  const saveProxyPreset = useSaveProxyPreset();
  const [testResult, setTestResult] = useState<{ success: boolean; message?: string | null; latency?: number } | null>(null);

  const form = useForm<ProxyConfig>({
    resolver: zodResolver(proxyConfigSchema),
    defaultValues: {
      type: (currentRow.settings?.proxy?.type as ProxyType) || ProxyType.ENVIRONMENT,
      url: currentRow.settings?.proxy?.url || '',
      username: currentRow.settings?.proxy?.username || '',
      password: currentRow.settings?.proxy?.password || '',
    },
  });

  const selectedProxyType = form.watch('type');

  const handlePresetSelect = (presetUrl: string) => {
    const preset = proxyPresets.find((p) => p.url === presetUrl);
    if (preset) {
      form.setValue('type', ProxyType.URL);
      form.setValue('url', preset.url);
      form.setValue('username', preset.username || '');
      form.setValue('password', preset.password || '');
    }
  };

  const onSubmit = async (values: ProxyConfig) => {
    try {
      // Prepare proxy config
      const proxyConfig = {
        type: values.type,
        ...(values.type === ProxyType.URL && {
          url: values.url,
          username: values.username || undefined,
          password: values.password || undefined,
        }),
      };

      const nextSettings = mergeChannelSettingsForUpdate(currentRow.settings, {
        proxy: proxyConfig,
      });

      await updateChannel.mutateAsync({
        id: currentRow.id,
        input: {
          settings: nextSettings,
        },
      });
      toast.success(m["channels.messages.updateSuccess"]());
      // Auto-save to proxy presets
      if (values.type === ProxyType.URL && values.url) {
        saveProxyPreset.mutate({ url: values.url, username: values.username || undefined, password: values.password || undefined });
      }
      onOpenChange(false);
    } catch (_error) {
      toast.error(m["channels.messages.updateError"]());
    }
  };

  const handleTest = async () => {
    setIsTesting(true);
    setTestResult(null);

    try {
      // Get current form values
      const values = form.getValues();

      // Prepare proxy config for testing
      const proxyConfig = {
        type: values.type,
        ...(values.type === ProxyType.URL && {
          url: values.url,
          username: values.username || undefined,
          password: values.password || undefined,
        }),
      };

      const result = await testChannel.mutateAsync({
        channelID: currentRow.id,
        modelID: currentRow.defaultTestModel,
        proxy: proxyConfig,
      });

      setTestResult({
        success: result.success,
        message: result.success ? result.message : result.error || 'Unknown error',
        latency: result.latency,
      });

      if (result.success) {
        toast.success(m["channels.dialogs.proxy.testSuccess"]());
      } else {
        toast.error(result.error || m["channels.dialogs.proxy.testFailed"]());
      }
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Unknown error';
      setTestResult({
        success: false,
        message: errorMessage,
      });
      toast.error(errorMessage);
    } finally {
      setIsTesting(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(state) => {
        if (!state) {
          form.reset();
          setTestResult(null);
        }
        onOpenChange(state);
      }}
    >
      <DialogContent className='sm:max-w-2xl'>
        <DialogHeader className='text-left'>
          <DialogTitle>{m["channels.dialogs.proxy.title"]()}</DialogTitle>
          <DialogDescription>{m["channels.dialogs.proxy.description"]({ name: currentRow.name })}</DialogDescription>
        </DialogHeader>

        <div className='space-y-6'>
          <Card>
            <CardHeader>
              <CardTitle className='text-lg'>{m["channels.dialogs.proxy.config.title"]()}</CardTitle>
              <CardDescription>{m["channels.dialogs.proxy.config.description"]()}</CardDescription>
            </CardHeader>
            <CardContent>
              <Form {...form}>
                <form className='space-y-4'>
                  <FormField
                    control={form.control}
                    name='type'
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{m["channels.dialogs.proxy.fields.type.label"]()}</FormLabel>
                        <Select onValueChange={field.onChange} value={field.value}>
                          <FormControl>
                            <SelectTrigger>
                              <SelectValue placeholder={m["channels.dialogs.proxy.fields.type.placeholder"]()} />
                            </SelectTrigger>
                          </FormControl>
                          <SelectContent>
                            <SelectItem value={ProxyType.DISABLED}>{m["channels.dialogs.proxy.types.disabled"]()}</SelectItem>
                            <SelectItem value={ProxyType.ENVIRONMENT}>{m["channels.dialogs.proxy.types.environment"]()}</SelectItem>
                            <SelectItem value={ProxyType.URL}>{m["channels.dialogs.proxy.types.url"]()}</SelectItem>
                          </SelectContent>
                        </Select>
                        <FormMessage />
                      </FormItem>
                    )}
                  />

                  {selectedProxyType === ProxyType.URL && proxyPresets.length > 0 && (
                    <FormItem>
                      <FormLabel>{m["channels.dialogs.proxy.presets.label"]()}</FormLabel>
                      <Select onValueChange={handlePresetSelect}>
                        <FormControl>
                          <SelectTrigger>
                            <SelectValue placeholder={m["channels.dialogs.proxy.presets.placeholder"]()} />
                          </SelectTrigger>
                        </FormControl>
                        <SelectContent>
                          {proxyPresets.map((preset) => (
                            <SelectItem key={preset.url} value={preset.url}>
                              {preset.url}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </FormItem>
                  )}

                  {selectedProxyType === ProxyType.URL && (
                    <>
                      <FormField
                        control={form.control}
                        name='url'
                        render={({ field }) => (
                          <FormItem>
                            <FormLabel>{m["channels.dialogs.proxy.fields.url.label"]()}</FormLabel>
                            <FormControl>
                              <Input placeholder={m["channels.dialogs.proxy.fields.url.placeholder"]()} {...field} />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />

                      <FormField
                        control={form.control}
                        name='username'
                        render={({ field }) => (
                          <FormItem>
                            <FormLabel>{m["channels.dialogs.proxy.fields.username.label"]()}</FormLabel>
                            <FormControl>
                              <Input placeholder={m["channels.dialogs.proxy.fields.username.placeholder"]()} {...field} />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />

                      <FormField
                        control={form.control}
                        name='password'
                        render={({ field }) => (
                          <FormItem>
                            <FormLabel>{m["channels.dialogs.proxy.fields.password.label"]()}</FormLabel>
                            <FormControl>
                              <Input type='password' placeholder={m["channels.dialogs.proxy.fields.password.placeholder"]()} {...field} />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                    </>
                  )}

                  {selectedProxyType === ProxyType.ENVIRONMENT && (
                    <div className='text-muted-foreground rounded-md border p-3 text-sm'>{m["channels.dialogs.proxy.environmentHint"]()}</div>
                  )}

                  {selectedProxyType === ProxyType.DISABLED && (
                    <div className='text-muted-foreground rounded-md border p-3 text-sm'>{m["channels.dialogs.proxy.disabledHint"]()}</div>
                  )}
                </form>
              </Form>
            </CardContent>
          </Card>

          {/* Test Result Card */}
          {testResult && (
            <Card className={testResult.success ? 'border-green-200' : 'border-red-200'}>
              <CardHeader>
                <CardTitle className={`text-lg ${testResult.success ? 'text-green-800' : 'text-red-800'}`}>
                  {testResult.success ? m["channels.dialogs.proxy.testSuccess"]() : m["channels.dialogs.proxy.testFailed"]()}
                </CardTitle>
              </CardHeader>
              <CardContent>
                {testResult.latency && (
                  <p className='text-sm'>
                    <span className='font-medium'>{m["channels.dialogs.proxy.latency"]()}:</span> {testResult.latency.toFixed(2)}s
                  </p>
                )}
                {testResult.message && (
                  <div className='mt-2'>
                    <ErrorDisplay error={testResult.message} />
                  </div>
                )}
              </CardContent>
            </Card>
          )}
        </div>

        <DialogFooter className='flex justify-between'>
          <Button type='button' variant='outline' onClick={handleTest} disabled={isTesting || testChannel.isPending}>
            <IconPlayerPlay className='mr-2 h-4 w-4' />
            {isTesting ? m["channels.dialogs.proxy.testing"]() : m["channels.dialogs.proxy.test"]()}
          </Button>
          <div className='flex gap-2'>
            <Button type='button' variant='outline' onClick={() => onOpenChange(false)}>
              {m["common.buttons.cancel"]()}
            </Button>
            <Button type='button' onClick={form.handleSubmit(onSubmit)} disabled={updateChannel.isPending}>
              {updateChannel.isPending ? m["common.buttons.saving"]() : m["common.buttons.save"]()}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
