import { zodResolver } from '@hookform/resolvers/zod';
import { HTMLAttributes } from 'react';
import { useForm } from 'react-hook-form';
import { z } from 'zod';

import { PasswordInput } from '@/components/password-input';
import { Button } from '@/components/ui/button';
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { useInitializeSystem } from '@/features/auth/data/initialization';
import { cn } from '@/lib/utils';
import * as m from '@/paraglide/messages';

type InitializationFormProps = HTMLAttributes<HTMLFormElement>;

// Create form schema factory to support i18n
const createFormSchema = () =>
  z.object({
    ownerEmail: z
      .string()
      .min(1, { message: m["initialization.form.validation.ownerEmailRequired"]() })
      .email({ message: m["initialization.form.validation.ownerEmailInvalid"]() }),
    ownerPassword: z
      .string()
      .min(1, {
        message: m["initialization.form.validation.ownerPasswordRequired"](),
      })
      .min(8, {
        message: m["initialization.form.validation.ownerPasswordMinLength"](),
      }),
    ownerFirstName: z.string().min(1, { message: m["initialization.form.validation.ownerFirstNameRequired"]() }),
    ownerLastName: z.string().min(1, { message: m["initialization.form.validation.ownerLastNameRequired"]() }),
    brandName: z.string().min(1, { message: m["initialization.form.validation.brandNameRequired"]() }),
  });

export function InitializationForm({ className, ...props }: InitializationFormProps) {
  const initializeSystemMutation = useInitializeSystem();

  const formSchema = createFormSchema(t);
  type FormData = z.infer<typeof formSchema>;

  const form = useForm<FormData>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      ownerEmail: '',
      ownerPassword: '',
      ownerFirstName: '',
      ownerLastName: '',
      brandName: '',
    },
  });

  function onSubmit(data: FormData) {
    const input = {
      ownerEmail: data.ownerEmail,
      ownerPassword: data.ownerPassword,
      ownerFirstName: data.ownerFirstName,
      ownerLastName: data.ownerLastName,
      brandName: data.brandName,
    };
    initializeSystemMutation.mutate(input);
  }

  return (
    <Form {...form}>
      <form onSubmit={form.handleSubmit(onSubmit)} className={cn('grid gap-4', className)} {...props}>
        <FormField
          control={form.control}
          name='ownerFirstName'
          render={({ field }) => (
            <FormItem>
              <FormLabel>{m["initialization.form.ownerFirstName"]()}</FormLabel>
              <FormControl>
                <Input
                  placeholder={m["initialization.form.placeholders.ownerFirstName"]()}
                  className='border-slate-300 !bg-white text-slate-800 transition-all duration-300 placeholder:text-slate-400 focus:border-slate-500 focus:!bg-white focus:ring-2 focus:ring-slate-200'
                  {...field}
                />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name='ownerLastName'
          render={({ field }) => (
            <FormItem>
              <FormLabel>{m["initialization.form.ownerLastName"]()}</FormLabel>
              <FormControl>
                <Input
                  placeholder={m["initialization.form.placeholders.ownerLastName"]()}
                  className='border-slate-300 !bg-white text-slate-800 transition-all duration-300 placeholder:text-slate-400 focus:border-slate-500 focus:!bg-white focus:ring-2 focus:ring-slate-200'
                  {...field}
                />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name='ownerEmail'
          render={({ field }) => (
            <FormItem>
              <FormLabel>{m["initialization.form.ownerEmail"]()}</FormLabel>
              <FormControl>
                <Input
                  placeholder={m["initialization.form.placeholders.ownerEmail"]()}
                  className='border-slate-300 !bg-white text-slate-800 transition-all duration-300 placeholder:text-slate-400 focus:border-slate-500 focus:!bg-white focus:ring-2 focus:ring-slate-200'
                  {...field}
                />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name='ownerPassword'
          render={({ field }) => (
            <FormItem>
              <FormLabel>{m["initialization.form.ownerPassword"]()}</FormLabel>
              <FormControl>
                <PasswordInput
                  placeholder={m["initialization.form.placeholders.ownerPassword"]()}
                  className='border-slate-300 bg-white text-slate-800 backdrop-blur-sm transition-all duration-300 placeholder:text-slate-400 focus:border-slate-500 focus:bg-white focus:ring-2 focus:ring-slate-200'
                  {...field}
                />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <FormField
          control={form.control}
          name='brandName'
          render={({ field }) => (
            <FormItem>
              <FormLabel>{m["initialization.form.brandName"]()}</FormLabel>
              <FormControl>
                <Input
                  placeholder={m["initialization.form.placeholders.brandName"]()}
                  className='border-slate-300 !bg-white text-slate-800 transition-all duration-300 placeholder:text-slate-400 focus:border-slate-500 focus:!bg-white focus:ring-2 focus:ring-slate-200'
                  {...field}
                />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />
        <Button
          type='submit'
          className='mt-6 w-full rounded-lg bg-slate-800 px-6 py-3 font-medium text-white shadow-lg transition-all duration-300 hover:bg-slate-700 hover:shadow-xl focus:ring-2 focus:ring-slate-500 focus:ring-offset-2 disabled:opacity-50'
          disabled={initializeSystemMutation.isPending}
        >
          {initializeSystemMutation.isPending ? m["initialization.form.submitting"]() : m["initialization.form.submit"]()}
        </Button>
      </form>
    </Form>
  );
}
