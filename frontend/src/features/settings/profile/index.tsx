
import ContentSection from '../components/content-section';
import ProfileForm from './profile-form';
import * as m from '@/paraglide/messages';

export default function SettingsProfile() {
  return (
    <ContentSection title={m["profile.title"]()} desc={m["profile.description"]()}>
      <ProfileForm />
    </ContentSection>
  );
}
