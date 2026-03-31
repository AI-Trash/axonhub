import { dynamicTranslation } from '@/lib/paraglide-helpers';
import * as m from '@/paraglide/messages';
import { zodResolver } from '@hookform/resolvers/zod';
import { toc } from '@lobehub/icons';
import { format } from 'date-fns';
import { CalendarIcon } from 'lucide-react';
import { useEffect, useState, useMemo, useCallback } from 'react';
import { useForm } from 'react-hook-form';

import { AutoComplete } from '@/components/auto-complete';
import { AutoCompleteSelect } from '@/components/auto-complete-select';
import { Button } from '@/components/ui/button';
import { Calendar } from '@/components/ui/calendar';
import { Checkbox } from '@/components/ui/checkbox';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Textarea } from '@/components/ui/textarea';
import { cn } from '@/lib/utils';
import { formatNumber } from '@/utils/format-number';

import { useModels } from '../context/models-context';
import { DEVELOPER_IDS, DEVELOPER_ICONS } from '../data/constants';
import { useCreateModel, useUpdateModel } from '../data/models';
import { useDevelopersData } from '../data/providers';
import { type Provider, type ProviderModel } from '../data/providers.schema';
import {
  CreateModelInput,
  createModelInputSchema,
  UpdateModelInput,
  ModelCard,
  ModelType,
  modelTypeSchema,
  updateModelInputSchema,
} from '../data/schema';

function isDeveloper(provider: string) {
  return DEVELOPER_IDS.includes(provider);
}

export function ModelsActionDialog() {
  const { open, setOpen, currentRow } = useModels();
  const createModel = useCreateModel();
  const updateModel = useUpdateModel();
  const { data: developersData } = useDevelopersData();
  const [selectedProvider, setSelectedProvider] = useState<string>('');
  const [developerSearchValue, setDeveloperSearchValue] = useState<string>('');
  const [modelIdInput, setModelIdInput] = useState<string>('');
  const [modelIdSearchValue, setModelIdSearchValue] = useState<string>('');
  const [_selectedModelCard, setSelectedModelCard] = useState<ModelCard>({});

  // 用于解决 Dialog 内 Popover 无法滚动的问题
  const [dialogContent, setDialogContent] = useState<HTMLDivElement | null>(null);

  const isEdit = open === 'edit';
  const isOpen = open === 'create' || open === 'edit';

  const providers = useMemo(() => {
    if (!developersData) return [];
    return Object.entries(developersData.providers)
      .filter(([key]) => isDeveloper(key))
      .map(([key, provider]: [string, Provider]) => ({
        id: key,
        name: provider.display_name || provider.name,
        models: provider.models || [],
      }));
  }, [developersData]);

  const selectedProviderModels = useMemo(() => {
    if (!selectedProvider) return [];
    const provider = providers.find((p) => p.id === selectedProvider);
    return provider?.models || [];
  }, [selectedProvider, providers]);

  const developerOptions = useMemo(() => {
    return DEVELOPER_IDS.map((id) => ({
      value: id,
      label: id,
    }));
  }, []);

  const modelIdOptions = useMemo(() => {
    return selectedProviderModels.map((m: ProviderModel) => ({
      value: m.id,
      label: m.id,
    }));
  }, [selectedProviderModels]);

  const iconOptions = useMemo(() => {
    return (
      Object.entries(toc)
        // @ts-ignore
        .filter(([_, value]) => value.group == 'provider' || value.group == 'model')
        .map(([_, value]) => ({
          // @ts-ignore
          value: value.id,
          // @ts-ignore
          label: value.id,
        }))
    );
  }, []);

  const form = useForm<CreateModelInput>({
    resolver: zodResolver(isEdit ? updateModelInputSchema : createModelInputSchema) as any,
    defaultValues: {
      developer: '',
      modelID: '',
      type: 'chat',
      name: '',
      icon: '',
      group: '',
      modelCard: {},
      settings: { associations: [] },
      remark: '',
    },
  });

  useEffect(() => {
    if (isEdit && currentRow) {
      form.reset({
        developer: currentRow.developer,
        modelID: currentRow.modelID,
        type: currentRow.type,
        name: currentRow.name,
        icon: currentRow.icon,
        group: currentRow.group,
        modelCard: currentRow.modelCard,
        settings: currentRow.settings,
        remark: currentRow.remark || '',
      });
      setSelectedProvider(currentRow.developer);
      setDeveloperSearchValue(currentRow.developer);
      setModelIdInput(currentRow.modelID);
      setModelIdSearchValue(currentRow.modelID);
      setSelectedModelCard(currentRow.modelCard || {});
    } else if (!isEdit) {
      form.reset({
        developer: '',
        modelID: '',
        type: 'chat',
        name: '',
        icon: '',
        group: '',
        modelCard: {},
        settings: { associations: [] },
        remark: '',
      });
      setSelectedProvider('');
      setDeveloperSearchValue('');
      setModelIdInput('');
      setModelIdSearchValue('');
      setSelectedModelCard({});
    }
  }, [isEdit, currentRow, form, isOpen]);

  const handleProviderChange = useCallback(
    (providerId: string) => {
      setSelectedProvider(providerId);
      setDeveloperSearchValue(providerId);
      form.setValue('developer', providerId);
      if (!isEdit) {
        const icon = DEVELOPER_ICONS[providerId] || providerId;
        form.setValue('icon', icon);
        setModelIdInput('');
        setModelIdSearchValue('');
        form.setValue('modelID', '');
        form.setValue('name', '');
        form.setValue('group', '');
        form.setValue('modelCard', {});
        setSelectedModelCard({});
      }
    },
    [form, isEdit]
  );

  const handleModelIdChange = useCallback(
    (modelId: string) => {
      setModelIdInput(modelId);
      setModelIdSearchValue(modelId);
      form.setValue('modelID', modelId);

      const selectedModel = selectedProviderModels.find((m: ProviderModel) => m.id === modelId);

      if (selectedModel && !isEdit) {
        form.setValue('name', selectedModel.display_name || selectedModel.name || '');
        form.setValue('group', selectedModel.family || selectedProvider);
        const normalizedType = selectedModel.type?.replace(/-/g, '_');
        if (normalizedType && modelTypeSchema.safeParse(normalizedType).success) {
          form.setValue('type', normalizedType as ModelType);
        }
        const modelCard: ModelCard = {
          reasoning: {
            supported: selectedModel.reasoning?.supported || false,
            default: selectedModel.reasoning?.default || false,
          },
          toolCall: selectedModel.tool_call,
          temperature: selectedModel.temperature,
          modalities: {
            input: selectedModel.modalities?.input || [],
            output: selectedModel.modalities?.output || [],
          },
          vision: selectedModel.vision,
          cost: {
            input: selectedModel.cost?.input || 0,
            output: selectedModel.cost?.output || 0,
            cacheRead: selectedModel.cost?.cache_read,
            cacheWrite: selectedModel.cost?.cache_write,
          },
          limit: {
            context: selectedModel.limit?.context || 0,
            output: selectedModel.limit?.output || 0,
          },
          knowledge: selectedModel.knowledge,
          releaseDate: selectedModel.release_date,
          lastUpdated: selectedModel.last_updated,
        };
        form.setValue('modelCard', modelCard);
        setSelectedModelCard(modelCard);
      } else {
        const currentModelCard = form.getValues('modelCard');
        setSelectedModelCard(currentModelCard || {});
      }
    },
    [selectedProviderModels, selectedProvider, form, isEdit]
  );

  const onSubmit = async (data: CreateModelInput) => {
    try {
      if (isEdit && currentRow) {
        const updateData: UpdateModelInput = {
          developer: data.developer,
          modelID: data.modelID,
          type: data.type,
          name: data.name,
          icon: data.icon,
          group: data.group,
          modelCard: data.modelCard,
          settings: data.settings,
          remark: data.remark,
        };
        await updateModel.mutateAsync({ id: currentRow.id, input: updateData });
      } else {
        await createModel.mutateAsync(data);
      }
      handleClose();
    } catch (_error) {
      // Error is handled by mutation
    }
  };

  const handleClose = useCallback(() => {
    setOpen(null);
    form.reset();
    setSelectedProvider('');
    setDeveloperSearchValue('');
    setModelIdInput('');
    setModelIdSearchValue('');
    setSelectedModelCard({});
  }, [form, setOpen]);

  return (
    <Dialog open={isOpen} onOpenChange={handleClose}>
      <DialogContent ref={setDialogContent} className='flex max-h-[90vh] flex-col overflow-hidden sm:max-w-6xl'>
        <DialogHeader className='flex-shrink-0 text-left'>
          <DialogTitle>{isEdit ? m["models.dialogs.edit.title"]() : m["models.dialogs.create.title"]()}</DialogTitle>
          <DialogDescription>{isEdit ? m["models.dialogs.edit.description"]() : m["models.dialogs.create.description"]()}</DialogDescription>
        </DialogHeader>

        <Form {...form}>
          <form id='model-form' onSubmit={form.handleSubmit(onSubmit)} className='flex min-h-0 flex-1 flex-col overflow-hidden'>
            <div className='flex min-h-0 flex-1 gap-6 overflow-x-auto overflow-y-hidden md:overflow-hidden'>
              {/* Left Panel - Basic Information */}
              <div className='min-h-0 w-1/2 flex-shrink-0 overflow-y-auto pr-4 md:w-1/3'>
                <div className='space-y-4'>
                  <FormField
                    control={form.control}
                    name='developer'
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{m["models.fields.developer"]()}</FormLabel>
                        <FormControl>
                          <AutoComplete
                            selectedValue={selectedProvider}
                            onSelectedValueChange={handleProviderChange}
                            searchValue={developerSearchValue}
                            onSearchValueChange={setDeveloperSearchValue}
                            items={developerOptions}
                            placeholder={m["models.fields.selectDeveloper"]()}
                            emptyMessage={m["models.fields.noModels"]()}
                            portalContainer={dialogContent}
                          />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />

                  <FormField
                    control={form.control}
                    name='modelID'
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{m["models.fields.modelId"]()}</FormLabel>
                        <FormControl>
                          {selectedProvider && modelIdOptions.length > 0 ? (
                            <AutoComplete
                              selectedValue={modelIdInput}
                              onSelectedValueChange={handleModelIdChange}
                              searchValue={modelIdSearchValue}
                              onSearchValueChange={setModelIdSearchValue}
                              items={modelIdOptions}
                              placeholder={m["models.fields.modelIdPlaceholder"]()}
                              emptyMessage={m["models.fields.noModels"]()}
                              portalContainer={dialogContent}
                            />
                          ) : (
                            <AutoComplete
                              selectedValue={modelIdInput}
                              onSelectedValueChange={handleModelIdChange}
                              searchValue={modelIdSearchValue}
                              onSearchValueChange={setModelIdSearchValue}
                              items={[]}
                              placeholder={m["models.fields.modelIdPlaceholder"]()}
                              emptyMessage={m["models.fields.noModels"]()}
                              portalContainer={dialogContent}
                            />
                          )}
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />

                  <FormField
                    control={form.control}
                    name='name'
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{m["models.fields.name"]()}</FormLabel>
                        <FormControl>
                          <Input {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />

                  <FormField
                    control={form.control}
                    name='icon'
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{m["models.fields.icon"]()}</FormLabel>
                        <FormControl>
                          <AutoCompleteSelect
                            selectedValue={field.value}
                            onSelectedValueChange={field.onChange}
                            items={iconOptions}
                            placeholder={m["models.fields.selectIcon"]()}
                            emptyMessage={m["models.fields.noIcons"]()}
                            portalContainer={dialogContent}
                          />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />

                  <FormField
                    control={form.control}
                    name='group'
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{m["models.fields.group"]()}</FormLabel>
                        <FormControl>
                          <Input {...field} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />

                  <FormField
                    control={form.control}
                    name='type'
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{m["models.fields.type"]()}</FormLabel>
                        <Select value={field.value} onValueChange={field.onChange}>
                          <FormControl>
                            <SelectTrigger>
                              <SelectValue />
                            </SelectTrigger>
                          </FormControl>
                          <SelectContent>
                            <SelectItem value='chat'>{m["models.types.chat"]()}</SelectItem>
                            <SelectItem value='embedding'>{m["models.types.embedding"]()}</SelectItem>
                            <SelectItem value='rerank'>{m["models.types.rerank"]()}</SelectItem>
                            <SelectItem value='image_generation'>{m["models.types.image_generation"]()}</SelectItem>
                            <SelectItem value='video_generation'>{m["models.types.video_generation"]()}</SelectItem>
                          </SelectContent>
                        </Select>
                        <FormMessage />
                      </FormItem>
                    )}
                  />

                  <FormField
                    control={form.control}
                    name='remark'
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>{m["models.fields.remark"]()}</FormLabel>
                        <FormControl>
                          <Textarea {...field} value={field.value || ''} />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                </div>
              </div>

              {/* Right Panel - Model Card Fields */}
              <div className='min-h-0 min-w-full flex-1 overflow-y-auto border-l pl-6 md:min-w-0'>
                <div className='space-y-4 pb-4'>
                  <h3 className='text-lg font-semibold'>{m["models.modelCard.title"]()}</h3>

                  <div className='space-y-2'>
                    <FormLabel>{m["models.modelCard.capabilities"]()}</FormLabel>
                    <div className='grid grid-cols-2 gap-2'>
                      <FormField
                        control={form.control}
                        name='modelCard.toolCall'
                        render={({ field }) => (
                          <FormItem className='flex items-center space-y-0 space-x-2'>
                            <FormControl>
                              <Checkbox checked={field.value || false} onCheckedChange={field.onChange} />
                            </FormControl>
                            <FormLabel className='font-normal'>{m["models.modelCard.toolCall"]()}</FormLabel>
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.vision'
                        render={({ field }) => (
                          <FormItem className='flex items-center space-y-0 space-x-2'>
                            <FormControl>
                              <Checkbox checked={field.value || false} onCheckedChange={field.onChange} />
                            </FormControl>
                            <FormLabel className='font-normal'>{m["models.modelCard.vision"]()}</FormLabel>
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.temperature'
                        render={({ field }) => (
                          <FormItem className='flex items-center space-y-0 space-x-2'>
                            <FormControl>
                              <Checkbox checked={field.value || false} onCheckedChange={field.onChange} />
                            </FormControl>
                            <FormLabel className='font-normal'>{m["models.modelCard.temperature"]()}</FormLabel>
                          </FormItem>
                        )}
                      />
                    </div>
                  </div>

                  <div className='space-y-2'>
                    <FormLabel>{m["models.modelCard.reasoning"]()}</FormLabel>
                    <div className='grid grid-cols-2 gap-2'>
                      <FormField
                        control={form.control}
                        name='modelCard.reasoning.supported'
                        render={({ field }) => (
                          <FormItem className='flex items-center space-y-0 space-x-2'>
                            <FormControl>
                              <Checkbox checked={field.value || false} onCheckedChange={field.onChange} />
                            </FormControl>
                            <FormLabel className='font-normal'>{m["models.modelCard.reasoningSupported"]()}</FormLabel>
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.reasoning.default'
                        render={({ field }) => (
                          <FormItem className='flex items-center space-y-0 space-x-2'>
                            <FormControl>
                              <Checkbox checked={field.value || false} onCheckedChange={field.onChange} />
                            </FormControl>
                            <FormLabel className='font-normal'>{m["models.modelCard.reasoningDefault"]()}</FormLabel>
                          </FormItem>
                        )}
                      />
                    </div>
                  </div>

                  <div className='space-y-2'>
                    <FormLabel>{m["models.modelCard.modalities"]()}</FormLabel>
                    <div className='grid grid-cols-2 gap-4'>
                      <FormField
                        control={form.control}
                        name='modelCard.modalities.input'
                        render={({ field }) => {
                          const modalityOptions = ['text', 'image', 'audio', 'video'];
                          return (
                            <FormItem>
                              <FormLabel className='text-xs'>{m["models.modelCard.input"]()}</FormLabel>
                              <div className='space-y-2'>
                                {modalityOptions.map((modality) => (
                                  <FormItem key={modality} className='flex items-center space-y-0 space-x-2'>
                                    <FormControl>
                                      <Checkbox
                                        checked={field.value?.includes(modality) || false}
                                        onCheckedChange={(checked) => {
                                          const current = field.value || [];
                                          if (checked) {
                                            field.onChange([...current, modality]);
                                          } else {
                                            field.onChange(current.filter((v) => v !== modality));
                                          }
                                        }}
                                      />
                                    </FormControl>
                                    <FormLabel className='font-normal'>{dynamicTranslation(`models.modelCard.${modality}`)}</FormLabel>
                                  </FormItem>
                                ))}
                              </div>
                              <FormMessage />
                            </FormItem>
                          );
                        }}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.modalities.output'
                        render={({ field }) => {
                          const modalityOptions = ['text', 'image', 'audio', 'video'];
                          return (
                            <FormItem>
                              <FormLabel className='text-xs'>{m["models.modelCard.output"]()}</FormLabel>
                              <div className='space-y-2'>
                                {modalityOptions.map((modality) => (
                                  <FormItem key={modality} className='flex items-center space-y-0 space-x-2'>
                                    <FormControl>
                                      <Checkbox
                                        checked={field.value?.includes(modality) || false}
                                        onCheckedChange={(checked) => {
                                          const current = field.value || [];
                                          if (checked) {
                                            field.onChange([...current, modality]);
                                          } else {
                                            field.onChange(current.filter((v) => v !== modality));
                                          }
                                        }}
                                      />
                                    </FormControl>
                                    <FormLabel className='font-normal'>{dynamicTranslation(`models.modelCard.${modality}`)}</FormLabel>
                                  </FormItem>
                                ))}
                              </div>
                              <FormMessage />
                            </FormItem>
                          );
                        }}
                      />
                    </div>
                  </div>

                  <div className='space-y-2'>
                    <FormLabel>{m["models.modelCard.cost"]()} ($/M tokens)</FormLabel>
                    <p className='text-muted-foreground text-xs'>{m["models.modelCard.costHint"]()}</p>
                    <div className='grid grid-cols-2 gap-2'>
                      <FormField
                        control={form.control}
                        name='modelCard.cost.input'
                        render={({ field }) => (
                          <FormItem>
                            <FormLabel className='text-xs'>{m["models.modelCard.input"]()}</FormLabel>
                            <FormControl>
                              <Input
                                type='number'
                                step='0.01'
                                {...field}
                                value={field.value ?? ''}
                                onChange={(e) => field.onChange(e.target.value ? Number(e.target.value) : undefined)}
                                placeholder='0'
                              />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.cost.output'
                        render={({ field }) => (
                          <FormItem>
                            <FormLabel className='text-xs'>{m["models.modelCard.output"]()}</FormLabel>
                            <FormControl>
                              <Input
                                type='number'
                                step='0.01'
                                {...field}
                                value={field.value ?? ''}
                                onChange={(e) => field.onChange(e.target.value ? Number(e.target.value) : undefined)}
                                placeholder='0'
                              />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.cost.cacheRead'
                        render={({ field }) => (
                          <FormItem>
                            <FormLabel className='text-xs'>{m["models.modelCard.cacheRead"]()}</FormLabel>
                            <FormControl>
                              <Input
                                type='number'
                                step='0.01'
                                {...field}
                                value={field.value ?? ''}
                                onChange={(e) => field.onChange(e.target.value ? Number(e.target.value) : undefined)}
                                placeholder='0'
                              />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.cost.cacheWrite'
                        render={({ field }) => (
                          <FormItem>
                            <FormLabel className='text-xs'>{m["models.modelCard.cacheWrite"]()}</FormLabel>
                            <FormControl>
                              <Input
                                type='number'
                                step='0.01'
                                {...field}
                                value={field.value ?? ''}
                                onChange={(e) => field.onChange(e.target.value ? Number(e.target.value) : undefined)}
                                placeholder='0'
                              />
                            </FormControl>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                    </div>
                  </div>

                  <div className='space-y-2'>
                    <FormLabel>{m["models.modelCard.limit"]()}</FormLabel>
                    <div className='grid grid-cols-2 gap-2'>
                      <FormField
                        control={form.control}
                        name='modelCard.limit.context'
                        render={({ field }) => {
                          return (
                            <FormItem>
                              <FormLabel className='text-xs'>{m["models.modelCard.context"]()}</FormLabel>
                              <FormControl>
                                <Input
                                  type='number'
                                  {...field}
                                  value={field.value || ''}
                                  onChange={(e) => field.onChange(e.target.value ? Number(e.target.value) : undefined)}
                                  placeholder='128000'
                                />
                              </FormControl>
                              {field.value && (
                                <p className='text-muted-foreground text-xs'>
                                  {m["models.modelCard.context"]()}: {formatNumber(field.value)}
                                </p>
                              )}
                              <FormMessage />
                            </FormItem>
                          );
                        }}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.limit.output'
                        render={({ field }) => {
                          return (
                            <FormItem>
                              <FormLabel className='text-xs'>{m["models.modelCard.output"]()}</FormLabel>
                              <FormControl>
                                <Input
                                  type='number'
                                  {...field}
                                  value={field.value || ''}
                                  onChange={(e) => field.onChange(e.target.value ? Number(e.target.value) : undefined)}
                                  placeholder='4096'
                                />
                              </FormControl>
                              {field.value && (
                                <p className='text-muted-foreground text-xs'>
                                  {m["models.modelCard.output"]()}: {formatNumber(field.value)}
                                </p>
                              )}
                              <FormMessage />
                            </FormItem>
                          );
                        }}
                      />
                    </div>
                  </div>

                  <div className='space-y-2'>
                    <FormLabel>{m["models.modelCard.dates"]()}</FormLabel>
                    <div className='grid grid-cols-3 gap-2'>
                      <FormField
                        control={form.control}
                        name='modelCard.knowledge'
                        render={({ field }) => (
                          <FormItem className='flex flex-col'>
                            <FormLabel className='text-xs'>{m["models.modelCard.knowledge"]()}</FormLabel>
                            <Popover>
                              <PopoverTrigger asChild>
                                <FormControl>
                                  <Button
                                    variant='outline'
                                    className={cn('w-full pl-3 text-left font-normal', !field.value && 'text-muted-foreground')}
                                  >
                                    {field.value ? (
                                      format(new Date(field.value.length === 7 ? `${field.value}-01` : field.value), 'yyyy-MM-dd')
                                    ) : (
                                      <span>Pick a date</span>
                                    )}
                                    <CalendarIcon className='ml-auto h-4 w-4 opacity-50' />
                                  </Button>
                                </FormControl>
                              </PopoverTrigger>
                              <PopoverContent className='w-auto p-0' align='start'>
                                <Calendar
                                  mode='single'
                                  selected={
                                    field.value ? new Date(field.value.length === 7 ? `${field.value}-01` : field.value) : undefined
                                  }
                                  onSelect={(date) => field.onChange(date ? format(date, 'yyyy-MM-dd') : undefined)}
                                  initialFocus
                                />
                              </PopoverContent>
                            </Popover>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.releaseDate'
                        render={({ field }) => (
                          <FormItem className='flex flex-col'>
                            <FormLabel className='text-xs'>{m["models.modelCard.releaseDate"]()}</FormLabel>
                            <Popover>
                              <PopoverTrigger asChild>
                                <FormControl>
                                  <Button
                                    variant='outline'
                                    className={cn('w-full pl-3 text-left font-normal', !field.value && 'text-muted-foreground')}
                                  >
                                    {field.value ? format(new Date(field.value), 'yyyy-MM-dd') : <span>Pick a date</span>}
                                    <CalendarIcon className='ml-auto h-4 w-4 opacity-50' />
                                  </Button>
                                </FormControl>
                              </PopoverTrigger>
                              <PopoverContent className='w-auto p-0' align='start'>
                                <Calendar
                                  mode='single'
                                  selected={field.value ? new Date(field.value) : undefined}
                                  onSelect={(date) => field.onChange(date ? format(date, 'yyyy-MM-dd') : undefined)}
                                  initialFocus
                                />
                              </PopoverContent>
                            </Popover>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                      <FormField
                        control={form.control}
                        name='modelCard.lastUpdated'
                        render={({ field }) => (
                          <FormItem className='flex flex-col'>
                            <FormLabel className='text-xs'>{m["models.modelCard.lastUpdated"]()}</FormLabel>
                            <Popover>
                              <PopoverTrigger asChild>
                                <FormControl>
                                  <Button
                                    variant='outline'
                                    className={cn('w-full pl-3 text-left font-normal', !field.value && 'text-muted-foreground')}
                                  >
                                    {field.value ? format(new Date(field.value), 'yyyy-MM-dd') : <span>Pick a date</span>}
                                    <CalendarIcon className='ml-auto h-4 w-4 opacity-50' />
                                  </Button>
                                </FormControl>
                              </PopoverTrigger>
                              <PopoverContent className='w-auto p-0' align='start'>
                                <Calendar
                                  mode='single'
                                  selected={field.value ? new Date(field.value) : undefined}
                                  onSelect={(date) => field.onChange(date ? format(date, 'yyyy-MM-dd') : undefined)}
                                  initialFocus
                                />
                              </PopoverContent>
                            </Popover>
                            <FormMessage />
                          </FormItem>
                        )}
                      />
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <div className='flex flex-shrink-0 justify-end gap-2 border-t pt-4'>
              <Button type='button' variant='outline' onClick={handleClose}>
                {m["common.buttons.cancel"]()}
              </Button>
              <Button type='submit' disabled={createModel.isPending || updateModel.isPending}>
                {isEdit ? m["common.buttons.save"]() : m["common.buttons.create"]()}
              </Button>
            </div>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
